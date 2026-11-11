#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod platform_linux;
#[cfg(target_os = "macos")]
mod platform_macos;
#[cfg(target_os = "linux")]
use platform_linux as platform;
#[cfg(target_os = "macos")]
use platform_macos as platform;

mod favicons;
mod tray_status;
mod updater_adapter;
mod updates;

use bar_core::{
    attention::AttentionTracker,
    limits,
    settings::{SettingKey, Settings, SettingsStore},
    threads, usage,
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};

static VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PANEL_HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(560);
static PANEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadItem {
    id: String,
    project_id: String,
    project_title: String,
    title: String,
    status: String,
    is_pinned: bool,
    providers: Vec<String>,
    branch: Option<String>,
    model_name: Option<String>,
    pr_number: Option<i64>,
    updated_at: String,
    latest_user_message_at: Option<String>,
    latest_run_completed_at: Option<String>,
    active_run_started_at: Option<String>,
    usage_limit_reset_at: Option<String>,
    project_icon: Option<String>,
    project_favicon: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Tokens {
    uncached_input: i64,
    cached_input: i64,
    cache_creation: i64,
    output: i64,
    reasoning: i64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct UsageReport {
    cost_usd: f64,
    processed_tokens: i64,
    tokens: Tokens,
    sessions: usize,
    records: usize,
    unpriced_records: usize,
    duplicates_dropped: usize,
    cache_savings_usd: f64,
    malformed_records: i64,
    rejected_files: usize,
    cache_modified: Option<String>,
    scan_cache_missing: bool,
    local_sources_partial: bool,
    cost_by_type: serde_json::Value,
    cost_by_speed: serde_json::Value,
    providers: Vec<serde_json::Value>,
    models: Vec<serde_json::Value>,
    timeline: Vec<serde_json::Value>,
    resolution: String,
    time_zone: String,
    until_ms: f64,
    notices: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LimitsReport {
    groups: Vec<serde_json::Value>,
    notices: Vec<String>,
    checked_at: Option<String>,
    links: Vec<serde_json::Value>,
    read_failed: bool,
    skipped_files: usize,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionState {
    status: String,
    message: String,
    connected: bool,
    paired: bool,
    has_data: bool,
    port: i64,
    updated_at: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateState {
    status: String,
    message: String,
    version: Option<String>,
    offer_id: Option<u64>,
    can_install: bool,
    busy: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSnapshot {
    startup_error: Option<String>,
    now: String,
    threads: Vec<ThreadItem>,
    online: bool,
    thread_error: Option<String>,
    loading: bool,
    connection: ConnectionState,
    usage: Option<UsageReport>,
    usage_error: Option<String>,
    usage_loading: bool,
    usage_period: String,
    limits: Option<LimitsReport>,
    settings: Settings,
    update: UpdateState,
    version: String,
}
impl Default for AppSnapshot {
    fn default() -> Self {
        Self {
            startup_error: None,
            now: Utc::now().to_rfc3339(),
            threads: vec![],
            online: false,
            thread_error: None,
            loading: true,
            connection: ConnectionState {
                status: "unavailable".into(),
                message: "T3 Code is unavailable. Showing saved data when present.".into(),
                connected: false,
                paired: false,
                has_data: false,
                port: 0,
                updated_at: None,
            },
            usage: None,
            usage_error: None,
            usage_loading: false,
            usage_period: "24h".into(),
            limits: None,
            settings: Settings::default(),
            update: UpdateState {
                status: "unavailable".into(),
                message: "Updates are unavailable in this build.".into(),
                version: None,
                offer_id: None,
                can_install: false,
                busy: false,
            },
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

#[derive(Clone, Default)]
struct ViewState {
    screen: String,
    metric: String,
    period: String,
    generation: u64,
}
struct RuntimeState {
    config: threads::Configuration,
    settings: Mutex<SettingsStore>,
    snapshot: Mutex<AppSnapshot>,
    view: Mutex<ViewState>,
    ids: Mutex<HashSet<String>>,
    attention: Mutex<AttentionTracker>,
    favicons: favicons::Favicons,
    usage: Mutex<usage::UsageReader>,
    limits: Mutex<limits::LimitsReader>,
    frozen_now: Option<DateTime<Utc>>,
    timezone: Tz,
    fixture: bool,
    fixture_notifications: bool,
    connection: limits::LimitsConnection,
    last_port: std::sync::atomic::AtomicU16,
    connection_generation: std::sync::atomic::AtomicU64,
    connection_gate: tokio::sync::Mutex<()>,
    thread_busy: std::sync::atomic::AtomicBool,
    usage_busy: std::sync::atomic::AtomicBool,
    limits_busy: std::sync::atomic::AtomicBool,
}
type SharedState = Arc<RuntimeState>;

fn now(state: &RuntimeState) -> DateTime<Utc> {
    state.frozen_now.unwrap_or_else(Utc::now)
}
pub fn record(event: &str, value: serde_json::Value) {
    eprintln!("[bar:{event}] {}", value);
}
fn record_failure(operation: &str, result: Result<(), String>) {
    if let Err(error) = result {
        record(
            "runtime_error",
            json!({"operation":operation,"error":error}),
        );
    }
}
fn fixture_or_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}
fn credential_store(
    fixture: bool,
    namespace: Option<&str>,
) -> Result<Arc<dyn limits::CredentialStore>, String> {
    match (fixture, namespace) {
        (false, None) => Ok(Arc::new(limits::KeyringStore::production())),
        (true, None) => Ok(Arc::new(FixtureCredentials::default())),
        (true, Some(raw)) => {
            let id = uuid::Uuid::parse_str(raw)
                .map_err(|_| "Fixture keyring namespace must be a UUID")?;
            if id.to_string() != raw || id.is_nil() {
                return Err("Fixture keyring namespace must be a nonzero canonical UUID".into());
            }
            Ok(Arc::new(limits::KeyringStore::new(
                format!("com.t3codebar.migration-probe.{id}"),
                "synthetic-only",
            )))
        }
        (false, Some(_)) => Err("Fixture keyring namespace requires fixture mode".into()),
    }
}
fn state_path() -> PathBuf {
    fixture_or_env("T3_BAR_FIXTURE_STATE")
        .map(PathBuf::from)
        .or_else(|| fixture_or_env("T3_CODE_BAR_STATE_DIR").map(PathBuf::from))
        .unwrap_or_else(|| threads::Configuration::from_environment(None).base_dir)
}
fn parse_now() -> Option<DateTime<Utc>> {
    fixture_or_env("T3_BAR_FROZEN_NOW")
        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&Utc))
}
fn parse_tz() -> Tz {
    fixture_or_env("T3_BAR_TIMEZONE")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            iana_time_zone::get_timezone()
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(chrono_tz::UTC)
        })
}
fn usage_period(raw: &str) -> usage::Period {
    match raw {
        "7d" => usage::Period::Week,
        "30d" => usage::Period::Month,
        "90d" => usage::Period::Quarter,
        _ => usage::Period::Day,
    }
}
fn panel_height(raw: f64) -> f64 {
    raw.clamp(80.0, 560.0)
}
fn settings_path(base: &Path) -> PathBuf {
    fixture_or_env("T3_BAR_FIXTURE_SETTINGS")
        .map(PathBuf::from)
        .unwrap_or_else(|| base.join("tauri-bar-settings.json"))
}

fn iso(d: Option<DateTime<Utc>>) -> Option<String> {
    d.map(|v| v.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
fn thread_item(t: threads::ThreadItem, state: &RuntimeState) -> ThreadItem {
    let project_favicon = if state.fixture {
        None
    } else {
        state.favicons.resolve(
            t.workspace_root.as_deref(),
            t.project_favicon_path.as_deref(),
        )
    };
    ThreadItem {
        project_favicon,
        id: t.id,
        project_id: t.project_id,
        project_title: t.project_title,
        title: t.title,
        status: format!("{:?}", t.status),
        is_pinned: t.is_pinned,
        providers: t.providers,
        branch: t.branch,
        model_name: t.model_name,
        pr_number: t.pr_number,
        updated_at: t
            .updated_at
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        latest_user_message_at: iso(t.latest_user_message_at),
        latest_run_completed_at: iso(t.latest_run_completed_at),
        active_run_started_at: iso(t.active_run_started_at),
        usage_limit_reset_at: iso(t.usage_limit_reset_at),
        project_icon: t.project_icon_json,
    }
}
fn provider_label(kind: &str) -> String {
    match kind {
        "codex" => "Codex".into(),
        "claudeAgent" | "claude" => "Claude".into(),
        "cursor" => "Cursor".into(),
        "grok" => "Grok".into(),
        "opencode" => "OpenCode".into(),
        "antigravity" => "Antigravity".into(),
        other => other.to_owned(),
    }
}

fn report_usage(r: usage::UsageReport, timezone: Tz) -> UsageReport {
    let providers = r.providers.iter().map(|p| json!({"kind":p.kind,"label":provider_label(&p.kind),"costUsd":p.cost_usd,"processedTokens":p.tokens.processed,"sessions":p.sessions,"records":p.records})).collect();
    let models = r.models.iter().map(|m| json!({"kind":m.kind,"name":m.name,"costUsd":m.cost_usd,"processedTokens":m.tokens.processed,"records":m.records,"unpricedRecords":m.unpriced_records})).collect();
    let timeline = r.timeline.columns.iter().map(|c| { let costs: Vec<_> = c.by_provider.iter().map(|(k,v)| json!({"kind":k,"costUsd":v.cost_usd,"processedTokens":v.tokens})).collect(); json!({"day":c.day,"hourStartMs":c.hour_start_ms,"label": if let Some(ms) = c.hour_start_ms { DateTime::from_timestamp_millis(ms as i64).map(|d| d.with_timezone(&timezone).format("%-I %p").to_string()).unwrap_or_default() } else { DateTime::from_timestamp(c.day * 86400, 0).ok_or(()).map(|d| d.format("%b %-d").to_string()).unwrap_or_default() }, "costUsd": costs.iter().filter_map(|v|v.get("costUsd").and_then(|x|x.as_f64())).sum::<f64>(), "processedTokens": costs.iter().filter_map(|v|v.get("processedTokens").and_then(|x|x.as_i64())).sum::<i64>(), "providers":costs}) }).collect();
    let tokens = Tokens {
        uncached_input: r.tokens.uncached_input,
        cached_input: r.tokens.cached_input,
        cache_creation: r.tokens.cache_creation,
        output: r.tokens.output,
        reasoning: r.tokens.reasoning,
    };
    UsageReport {
        cost_usd: r.cost_usd,
        processed_tokens: r.tokens.processed,
        tokens,
        sessions: r.sessions,
        records: r.records,
        unpriced_records: r.unpriced_records,
        duplicates_dropped: r.duplicates_dropped,
        cache_savings_usd: r.cache_savings_usd,
        malformed_records: r.malformed_records,
        rejected_files: r.rejected_files,
        cache_modified: iso(r.cache_modified),
        scan_cache_missing: r.scan_cache_missing,
        local_sources_partial: r
            .local_sources
            .iter()
            .any(|source| source.state == "partial" || source.state == "unreadable"),
        cost_by_type: serde_json::to_value(r.cost_by_type).unwrap_or_default(),
        cost_by_speed: serde_json::to_value(r.cost_by_speed).unwrap_or_default(),
        providers,
        models,
        timeline,
        resolution: r.timeline.resolution,
        time_zone: r.timeline.timezone,
        until_ms: r.timeline.until_ms,
        notices: r.notices,
    }
}
fn report_limits(s: limits::LimitsSnapshot, now: DateTime<Utc>, read_failed: bool) -> LimitsReport {
    let groups = s.groups(now).into_iter().map(|g| { let rows = g.rows.into_iter().map(|r| json!({"id":r.id,"label":r.label,"remainingPercent":r.remaining_percent,"usedPercent":r.used_percent,"resetAt":r.resets.first().map(|x|x.at.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)),"pace":r.pace.map(|p| format!("{:?}",p).to_lowercase()),"members":r.members.into_iter().map(|m| json!({"label":m.account.display_name.unwrap_or(m.account.id),"remainingPercent":m.window.left_percent(),"resetAt":m.window.resets_at.map(|x|x.to_rfc3339_opts(chrono::SecondsFormat::Millis,true)),"plan":m.account.plan,"evenPaceLeft":m.window.even_pace_left(now),"refillPercent":m.refill_percent,"resetCredits":m.account.reset_credits.as_ref().map(|c|c.available_count)})).collect::<Vec<_>>() })).collect::<Vec<_>>(); json!({"driver":g.driver,"label":provider_label(&g.driver),"rows":rows}) }).collect();
    let links = s
        .links
        .iter()
        .filter(|l| l.url.starts_with("https://") || l.url.starts_with("http://"))
        .map(|l| json!({"label":l.label,"url":l.url,"message":l.message,"accounts":l.accounts}))
        .collect();
    LimitsReport {
        groups,
        notices: s.notices,
        checked_at: s
            .accounts
            .iter()
            .map(|a| a.checked_at)
            .max()
            .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        links,
        read_failed,
        skipped_files: s.skipped_files,
    }
}
fn connection_for(state: &RuntimeState, online: bool, has_data: bool) -> ConnectionState {
    let live = state.connection.snapshot();
    let (status, message) = match &live.status {
        limits::LiveStatus::Live => ("connected", "Connected to T3 Code".to_owned()),
        limits::LiveStatus::Connecting => ("connecting", "Connecting to T3 Code…".to_owned()),
        limits::LiveStatus::Retrying => (
            "retrying",
            "Reconnecting. Showing the last available data.".to_owned(),
        ),
        limits::LiveStatus::Failed(message) => ("failed", message.clone()),
        _ if !online => (
            "offline",
            "Open T3 Code to connect. Showing saved data when present.".to_owned(),
        ),
        _ => (
            "cacheOnly",
            "Showing saved data. Connect to receive live limits.".to_owned(),
        ),
    };
    ConnectionState {
        status: status.into(),
        message,
        connected: live.status == limits::LiveStatus::Live,
        paired: live.has_credential,
        has_data,
        port: state.config.server_port().map(i64::from).unwrap_or(0),
        updated_at: iso(live.updated_at),
    }
}
fn emit_snapshot(app: &tauri::AppHandle, state: &RuntimeState) {
    if let Ok(s) = state.snapshot.lock().map(|x| x.clone()) {
        let _ = app.emit("bar-snapshot", s);
    }
}
fn refresh_connection(state: &SharedState, online: bool) {
    let port = if online {
        state.config.server_port().unwrap_or(0)
    } else {
        0
    };
    let previous = state
        .last_port
        .swap(port, std::sync::atomic::Ordering::AcqRel);
    if previous == port {
        return;
    }
    let state = state.clone();
    let generation = begin_connection_operation(&state);
    tauri::async_runtime::spawn(async move {
        let _gate = state.connection_gate.lock().await;
        if !connection_operation_current(&state, generation) {
            return;
        }
        let connection = &state.connection;
        if port == 0 {
            connection.pause().await;
        } else {
            if previous != 0 {
                connection.pause().await;
            }
            let _ = connection.reconnect_saved_if_available(port).await;
        }
    });
}
fn refresh_threads(app: &tauri::AppHandle, state: &SharedState) {
    if state
        .thread_busy
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    let app = app.clone();
    let state = state.clone();
    thread::spawn(move || {
        let result = threads::read_threads(&state.config, now(&state));
        match &result {
            Ok(value) => tray_status::update(&app, &value.summary),
            Err(_) => tray_status::update(&app, &threads::ThreadSummary::new(&[], false)),
        }
        let mut alerts = Vec::new();
        if let Ok(mut snap) = state.snapshot.lock() {
            match result {
                Ok(v) => {
                    alerts = state.attention.lock().unwrap().update(
                        v.threads
                            .iter()
                            .map(|t| (t.id.as_str(), t.title.as_str(), t.status)),
                        snap.settings.notifications_muted
                            || !state.connection.snapshot().has_credential
                            || (state.fixture && !state.fixture_notifications),
                    );
                    snap.threads = v
                        .threads
                        .into_iter()
                        .map(|t| thread_item(t, &state))
                        .collect();
                    snap.online = v.is_server_online;
                    snap.thread_error = None;
                    *state.ids.lock().unwrap() =
                        snap.threads.iter().map(|t| t.id.clone()).collect();
                }
                Err(e) => {
                    snap.online = false;
                    snap.thread_error = Some(e.to_string());
                }
            }
            snap.now = now(&state).to_rfc3339();
            snap.loading = false;
            refresh_connection(&state, snap.online);
            snap.connection = connection_for(
                &state,
                snap.online,
                !snap.threads.is_empty()
                    || snap
                        .limits
                        .as_ref()
                        .is_some_and(|l| !l.groups.is_empty() || !l.links.is_empty())
                    || state.config.base_dir.join("statev2.sqlite").exists(),
            );
        }
        for alert in alerts {
            use tauri_plugin_notification::NotificationExt;
            let notification = app
                .notification()
                .builder()
                .title(alert.title())
                .body(&alert.body)
                .extra("threadId", alert.thread_id);
            #[cfg(target_os = "macos")]
            let notification = notification.sound("NSUserNotificationDefaultSoundName");
            if let Err(error) = notification.show() {
                record("notification_error", json!({"error":error.to_string()}));
            }
        }
        state
            .thread_busy
            .store(false, std::sync::atomic::Ordering::Release);
        emit_snapshot(&app, &state);
    });
}
fn refresh_limits(app: &tauri::AppHandle, state: &SharedState, read_cache: bool) {
    if state
        .limits_busy
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    let app = app.clone();
    let state = state.clone();
    thread::spawn(move || {
        if let Ok(mut reader) = state.limits.lock() {
            let cached = if read_cache {
                Some(reader.read(&state.config.base_dir, now(&state)))
            } else {
                None
            };
            let merged = reader.merge_live(&state.connection);
            if let Ok(mut snap) = state.snapshot.lock() {
                if let Some(data) =
                    merged.or_else(|| cached.as_ref().and_then(|r| r.as_ref().ok()).cloned())
                {
                    snap.limits = Some(report_limits(
                        data,
                        now(&state),
                        cached.as_ref().is_some_and(|r| r.is_err()),
                    ));
                } else if let Some(Err(_)) = cached {
                    if let Some(data) = &mut snap.limits {
                        data.read_failed = true;
                    }
                }
            }
        }
        state
            .limits_busy
            .store(false, std::sync::atomic::Ordering::Release);
        emit_snapshot(&app, &state);
    });
}
fn refresh_visible(app: &tauri::AppHandle, state: &SharedState) {
    if !VISIBLE.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let view = state.view.lock().map(|v| v.clone()).unwrap_or_default();
    if view.screen != "usage" {
        return;
    }
    if view.metric != "limits" {
        if state
            .usage_busy
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let period = usage_period(&view.period);
        let app = app.clone();
        let state = state.clone();
        thread::spawn(move || {
            let result = state
                .usage
                .lock()
                .map_err(|_| usage::UsageError::Unreadable)
                .and_then(|mut r| r.read(period, now(&state), state.timezone));
            let current = state
                .view
                .lock()
                .map(|v| {
                    v.generation == view.generation
                        && v.screen == "usage"
                        && v.metric != "limits"
                        && VISIBLE.load(std::sync::atomic::Ordering::Acquire)
                })
                .unwrap_or(false);
            if current {
                if let Ok(v) = result {
                    if let Ok(mut s) = state.snapshot.lock() {
                        s.usage = Some(report_usage(v, state.timezone));
                        s.usage_period = view.period.clone();
                        s.usage_error = None;
                        s.usage_loading = false;
                    }
                } else if let Ok(mut s) = state.snapshot.lock() {
                    s.usage_loading = false;
                    match result.unwrap_err() {
                        usage::UsageError::Missing => {
                            s.usage = None;
                            s.usage_error =
                                Some("No usage data yet. Open T3 Code to collect usage.".into());
                        }
                        usage::UsageError::UnsupportedVersion(_) => {
                            s.usage = None;
                            s.usage_error = Some("T3 Code's usage format has changed. Update T3 Code Bar to read it.".into());
                        }
                        usage::UsageError::Unreadable => {
                            s.usage_error = Some(
                                "Could not read usage. Showing the last successful scan.".into(),
                            );
                        }
                    }
                }
            }
            state
                .usage_busy
                .store(false, std::sync::atomic::Ordering::Release);
            emit_snapshot(&app, &state);
        });
    }
}
fn poller(app: tauri::AppHandle, state: SharedState) {
    thread::spawn(move || {
        {
            let _gate = state.connection_gate.blocking_lock();
            if let Err(error) = state.connection.restore_saved() {
                record(
                    "connection_restore_error",
                    json!({"error": error.to_string()}),
                );
            }
        }
        let mut tick = 0u64;
        loop {
            refresh_threads(&app, &state);
            refresh_visible(&app, &state);
            refresh_limits(
                &app,
                &state,
                tick == 0
                    || (tick.is_multiple_of(5)
                        && VISIBLE.load(std::sync::atomic::Ordering::Acquire)),
            );
            tick = tick.wrapping_add(1);
            thread::sleep(Duration::from_secs(1));
        }
    });
}
pub fn panel_opened(app: &tauri::AppHandle) {
    if !VISIBLE.swap(true, std::sync::atomic::Ordering::AcqRel) {
        if let Some(state) = app.try_state::<SharedState>() {
            if let Ok(mut view) = state.view.lock() {
                view.screen = "threads".into();
                view.generation = view.generation.wrapping_add(1);
            }
        }
        let _ = app.emit("bar-opened", ());
        updates::show_pending(app);
    }
}
pub fn panel_closed(app: &tauri::AppHandle) {
    VISIBLE.store(false, std::sync::atomic::Ordering::Release);
    if let Some(state) = app.try_state::<SharedState>() {
        if let Ok(mut view) = state.view.lock() {
            view.screen = "threads".into();
            view.generation = view.generation.wrapping_add(1);
        }
    }
}
#[derive(Default)]
struct FixtureCredentials(Mutex<Option<limits::Credential>>);
impl limits::CredentialStore for FixtureCredentials {
    fn load(&self) -> Result<Option<limits::Credential>, limits::ConnectionError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, credential: &limits::Credential) -> Result<(), limits::ConnectionError> {
        *self.0.lock().unwrap() = Some(credential.clone());
        Ok(())
    }
    fn delete(&self) -> Result<(), limits::ConnectionError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[tauri::command]
fn get_snapshot(state: State<'_, SharedState>) -> Result<AppSnapshot, String> {
    state
        .snapshot
        .lock()
        .map(|s| s.clone())
        .map_err(|_| "snapshot unavailable".into())
}
#[tauri::command]
fn set_view(
    screen: Option<String>,
    metric: Option<String>,
    period: Option<String>,
    state: State<'_, SharedState>,
) -> Result<(), String> {
    let mut v = state.view.lock().map_err(|_| "view unavailable")?;
    if let Some(s) = screen {
        if ["threads", "usage", "settings"].contains(&s.as_str()) {
            v.screen = s;
        }
    }
    if let Some(m) = metric {
        if ["cost", "tokens", "limits"].contains(&m.as_str()) {
            v.metric = m;
        }
    }
    if let Some(p) = period {
        if ["24h", "7d", "30d", "90d"].contains(&p.as_str()) {
            v.period = p;
        }
    }
    v.generation = v.generation.wrapping_add(1);
    Ok(())
}
#[tauri::command]
fn resize_panel(height: f64, window: tauri::Window) -> Result<(), String> {
    if !height.is_finite() {
        return Err("Invalid panel height".into());
    }
    let h = panel_height(height).ceil() as u32;
    if PANEL_HEIGHT.swap(h, std::sync::atomic::Ordering::AcqRel) == h {
        return Ok(());
    }
    platform::resize(&window, h)
}
#[tauri::command]
fn hide_panel(window: tauri::Window) -> Result<(), String> {
    platform::hide(&window)
}
#[tauri::command]
fn set_setting(
    key: SettingKey,
    value: bool,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|_| "settings unavailable")?;
    settings.set(key, value).map_err(|e| e.to_string())?;
    let next = settings.value().clone();
    drop(settings);
    if let Ok(mut snap) = state.snapshot.lock() {
        snap.settings = next;
    }
    let _ = app.emit("bar-settings", ());
    emit_snapshot(&app, &state);
    Ok(())
}
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}
#[tauri::command]
fn thread_action(
    id: String,
    action: String,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    if !state
        .ids
        .lock()
        .map_err(|_| "threads unavailable")?
        .contains(&id)
    {
        return Err("Unknown thread".into());
    }
    let url = state
        .config
        .thread_url(&id, action == "browser")
        .map_err(|e| e.to_string())?;
    match action.as_str() {
        "copy" => {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            app.clipboard()
                .write_text(&url)
                .map_err(|e| e.to_string())?;
        }
        "open" | "browser" => {
            use tauri_plugin_opener::OpenerExt;
            app.opener()
                .open_url(url, None::<String>)
                .map_err(|e| e.to_string())?;
        }
        _ => return Err("Unknown thread action".into()),
    }
    Ok(())
}
#[tauri::command]
fn new_thread(app: tauri::AppHandle, _state: State<'_, SharedState>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url("t3code://new", None::<String>)
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn get_preview(
    id: String,
    state: State<'_, SharedState>,
) -> Result<Option<threads::ThreadPreview>, String> {
    if !state
        .snapshot
        .lock()
        .map_err(|_| "snapshot unavailable")?
        .settings
        .shows_previews
    {
        return Ok(None);
    }
    if !state
        .ids
        .lock()
        .map_err(|_| "threads unavailable")?
        .contains(&id)
    {
        return Err("Unknown thread".into());
    }
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        if state.fixture {
            let delay = fixture_or_env("T3_BAR_FIXTURE_PREVIEW_DELAY_MS")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
                .min(5000);
            record("fixture_preview_started", json!({"id":id,"delayMs":delay}));
            thread::sleep(Duration::from_millis(delay));
        }
        let result = threads::read_preview(&state.config, &id).map_err(|e| e.to_string());
        if state.fixture {
            record(
                "fixture_preview_completed",
                json!({"id":id,"ok":result.is_ok()}),
            );
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn open_limit_link(
    url: String,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let snap = state.snapshot.lock().map_err(|_| "snapshot unavailable")?;
    let allowed = snap
        .limits
        .as_ref()
        .map(|l| {
            l.links
                .iter()
                .any(|v| v.get("url").and_then(|u| u.as_str()) == Some(url.as_str()))
        })
        .unwrap_or(false);
    if !allowed {
        return Err("Link is not in the current limits snapshot".into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<String>)
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn check_for_updates(app: tauri::AppHandle) -> Result<(), String> {
    updates::check(&app, bar_updater::CheckKind::Manual).await;
    Ok(())
}
#[tauri::command]
async fn update_prompt_shown(app: tauri::AppHandle, offer_id: u64, version: String) {
    updates::acknowledge_prompt(&app, offer_id, &version).await;
}
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    offer_id: u64,
    version: String,
) -> Result<(), String> {
    updates::install(&app, offer_id, &version).await
}
fn begin_connection_operation(state: &RuntimeState) -> u64 {
    state
        .connection_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
        .wrapping_add(1)
}
fn connection_operation_current(state: &RuntimeState, generation: u64) -> bool {
    state
        .connection_generation
        .load(std::sync::atomic::Ordering::Acquire)
        == generation
}
fn open_notification_thread(
    config: &threads::Configuration,
    ids: &Mutex<HashSet<String>>,
    id: &str,
    open: impl FnOnce(String) -> Result<(), String>,
) -> Result<(), String> {
    if !ids.lock().map_err(|_| "threads unavailable")?.contains(id) {
        return Err("Notification thread is no longer available".into());
    }
    let url = config.thread_url(id, false).map_err(|e| e.to_string())?;
    open(url)
}
#[tauri::command]
async fn connect(
    token: String,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let generation = begin_connection_operation(&state);
    let port = state.config.server_port().map_err(|e| e.to_string())?;
    let _gate = state.connection_gate.lock().await;
    if !connection_operation_current(&state, generation) {
        return Ok(());
    }
    let result = state
        .connection
        .connect(port, &token)
        .await
        .map_err(|e| e.to_string());
    refresh_threads(&app, &state);
    result
}
#[tauri::command]
async fn reconnect(state: State<'_, SharedState>, app: tauri::AppHandle) -> Result<(), String> {
    let generation = begin_connection_operation(&state);
    let port = state.config.server_port().map_err(|e| e.to_string())?;
    let _gate = state.connection_gate.lock().await;
    if !connection_operation_current(&state, generation) {
        return Ok(());
    }
    let result = state
        .connection
        .reconnect_saved(port)
        .await
        .map_err(|e| e.to_string());
    refresh_threads(&app, &state);
    result
}
#[tauri::command]
async fn disconnect(state: State<'_, SharedState>, app: tauri::AppHandle) -> Result<(), String> {
    begin_connection_operation(&state);
    let _gate = state.connection_gate.lock().await;
    let result = state
        .connection
        .disconnect()
        .await
        .map_err(|e| e.to_string());
    refresh_limits(&app, &state, true);
    refresh_threads(&app, &state);
    result
}

fn make_tray(app: &mut tauri::App) -> tauri::Result<()> {
    if cfg!(target_os = "linux") {
        return Ok(());
    }
    let show = MenuItem::with_id(app, "show", "Show T3 Code Bar", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit T3 Code Bar", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &settings, &quit])?;
    TrayIconBuilder::with_id("bar-tray")
        .icon(tauri::include_image!("icons/icon.png"))
        .menu(&menu)
        .tooltip("T3 Code Bar")
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                platform::toggle(app);
            }
        })
        .on_menu_event(|app, e| match e.id.as_ref() {
            "show" => {
                record_failure("tray_show", platform::show(app));
            }
            "settings" => {
                record_failure("tray_settings_show", platform::show(app));
                record_failure(
                    "tray_settings_event",
                    app.emit("bar-settings", ()).map_err(|e| e.to_string()),
                );
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    tray_status::update(app.handle(), &threads::ThreadSummary::new(&[], false));
    Ok(())
}
fn main() {
    platform::prepare_runtime();
    let base = state_path();
    let config = threads::Configuration {
        base_dir: base.clone(),
    };
    let fixture = fixture_or_env("T3_BAR_FIXTURE_STATE").is_some();
    let settings_file = settings_path(&base);
    let (settings, startup_error) = match SettingsStore::load(settings_file.clone()) {
        Ok(settings) => (settings, None),
        Err(error) => {
            eprintln!(
                "Cannot load settings; keeping the existing file and using defaults: {error}"
            );
            (SettingsStore::defaults(settings_file), Some("Settings could not be loaded. Using defaults for this session; the original file is preserved and setting changes cannot be saved.".into()))
        }
    };
    let namespace = fixture_or_env("T3_BAR_FIXTURE_KEYRING_NAMESPACE");
    let store = match credential_store(fixture, namespace.as_deref()) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("Cannot configure credential storage: {error}");
            std::process::exit(1);
        }
    };
    let mut usage_reader = usage::UsageReader::new(base.clone());
    if !fixture {
        if let Some(home) = fixture_or_env("HOME") {
            usage_reader.set_local_roots(usage::LocalRoots::discover(&base, PathBuf::from(home)));
        }
    }

    let state = Arc::new(RuntimeState {
        config: config.clone(),
        snapshot: Mutex::new(AppSnapshot {
            startup_error,
            settings: settings.value().clone(),
            ..Default::default()
        }),
        settings: Mutex::new(settings),
        view: Mutex::new(ViewState::default()),
        ids: Mutex::new(HashSet::new()),
        attention: Mutex::new(AttentionTracker::default()),
        favicons: Default::default(),
        usage: Mutex::new(usage_reader),
        limits: Mutex::new(limits::LimitsReader::new()),
        frozen_now: parse_now(),
        timezone: parse_tz(),
        fixture,
        fixture_notifications: fixture
            && fixture_or_env("T3_BAR_FIXTURE_NOTIFICATIONS").as_deref() == Some("1"),
        connection: limits::LimitsConnection::with_default_transport(store),
        last_port: Default::default(),
        connection_generation: Default::default(),
        connection_gate: tokio::sync::Mutex::new(()),
        thread_busy: Default::default(),
        usage_busy: Default::default(),
        limits_busy: Default::default(),
    });
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            record_failure("single_instance_show", platform::show(app));
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            set_view,
            resize_panel,
            hide_panel,
            set_setting,
            quit_app,
            thread_action,
            new_thread,
            get_preview,
            open_limit_link,
            check_for_updates,
            install_update,
            update_prompt_shown,
            connect,
            reconnect,
            disconnect
        ]);
    let result = builder
        .setup(move |app| {
            make_tray(app)?;
            platform::initialize(app)?;
            use tauri_plugin_notification::NotificationExt;
            let handle = app.handle().clone();
            app.notification().on_action(move |event| {
                if event.action_id() != "tap" {
                    return;
                }
                let Some(id) = event
                    .notification()
                    .and_then(|n| n.extra().get("threadId"))
                    .and_then(|v| v.as_str())
                else {
                    return;
                };
                let state = handle.state::<SharedState>();
                if state.fixture {
                    if state.fixture_notifications {
                        record("fixture_notification_tap", json!({"threadId":id}));
                    }
                    return;
                }
                if let Err(error) = open_notification_thread(&state.config, &state.ids, id, |url| {
                    use tauri_plugin_opener::OpenerExt;
                    handle
                        .opener()
                        .open_url(url, None::<String>)
                        .map_err(|e| e.to_string())
                }) {
                    record("notification_open_error", json!({"error":error}));
                }
            })?;
            updates::initialize(app.handle());
            if state.fixture && fixture_or_env("T3_BAR_FIXTURE_OPEN").as_deref() == Some("1") {
                let handle = app.handle().clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(500));
                    let target = handle.clone();
                    let _ = handle.run_on_main_thread(move || {
                        let _ = platform::show(&target);
                    });
                });
            }
            poller(app.handle().clone(), state.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                record_failure("close_hide", platform::hide(window));
            }
            platform::on_window_event(window, event);
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("T3 Code Bar failed to start: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notification_routes_only_current_threads_and_preserves_opener_errors() {
        let directory =
            std::env::temp_dir().join(format!("t3-notification-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("environment-id"), "fixture-environment").unwrap();
        let config = threads::Configuration {
            base_dir: directory.clone(),
        };
        let ids = Mutex::new(HashSet::from(["thread-1".into()]));
        let result = open_notification_thread(&config, &ids, "missing", |_| {
            panic!("unknown thread opened")
        });
        assert_eq!(
            result,
            Err("Notification thread is no longer available".into())
        );
        let mut routed = None;
        open_notification_thread(&config, &ids, "thread-1", |url| {
            routed = Some(url);
            Ok(())
        })
        .unwrap();
        assert_eq!(routed, Some(config.thread_url("thread-1", false).unwrap()));
        assert_eq!(
            open_notification_thread(&config, &ids, "thread-1", |_| Err(
                "opener unavailable".into()
            )),
            Err("opener unavailable".into())
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn usage_timeline_metadata_survives_host_mapping() {
        use chrono::TimeZone;
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/usage/state");
        let dataset =
            usage::decode_cache(&std::fs::read(root.join("usage-scan-cache-v11.json")).unwrap())
                .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
        for period in [
            usage::Period::Day,
            usage::Period::Week,
            usage::Period::Month,
            usage::Period::Quarter,
        ] {
            let window = usage::Window::new(period, now, chrono_tz::Asia::Ho_Chi_Minh);
            let mut source =
                usage::UsageReport::build(&dataset, &window, &usage::PriceBook::load(&root));
            source.malformed_records = 2;
            source.rejected_files = 1;
            source.cache_modified = Some(now);
            source.scan_cache_missing = false;
            source.local_sources.push(usage::LocalStatus {
                kind: "opencode".into(),
                state: "partial".into(),
                checked_at: now,
                data_updated_at: None,
                failed_files: 1,
            });
            let expected = source.timeline.clone();
            let mapped =
                serde_json::to_value(report_usage(source, chrono_tz::Asia::Ho_Chi_Minh)).unwrap();
            assert_eq!(mapped["malformedRecords"], 2);
            assert_eq!(mapped["rejectedFiles"], 1);
            assert_eq!(mapped["cacheModified"], "2026-11-06T09:08:00.000Z");
            assert_eq!(mapped["scanCacheMissing"], false);
            assert_eq!(mapped["localSourcesPartial"], true);
            assert_eq!(mapped["resolution"], expected.resolution);
            assert_eq!(mapped["timeZone"], expected.timezone);
            assert_eq!(mapped["untilMs"], expected.until_ms);
            assert_eq!(
                mapped["timeline"].as_array().unwrap().len(),
                expected.columns.len()
            );
            for (column, original) in mapped["timeline"]
                .as_array()
                .unwrap()
                .iter()
                .zip(expected.columns)
            {
                assert_eq!(column["day"], original.day);
                assert_eq!(
                    column["hourStartMs"],
                    serde_json::json!(original.hour_start_ms)
                );
            }
        }
    }

    #[test]
    fn runtime_destination_change_invalidates_an_inflight_connection() {
        use std::sync::atomic::Ordering;
        let root = std::env::temp_dir().join(format!("t3-bar-host-test-{}", uuid::Uuid::new_v4()));
        let settings = SettingsStore::load(root.join("settings.json")).unwrap();
        let state = Arc::new(RuntimeState {
            config: threads::Configuration {
                base_dir: root.clone(),
            },
            settings: Mutex::new(settings),
            snapshot: Mutex::new(AppSnapshot::default()),
            view: Mutex::new(ViewState::default()),
            ids: Mutex::new(HashSet::new()),
            attention: Mutex::new(AttentionTracker::default()),
            favicons: Default::default(),
            usage: Mutex::new(usage::UsageReader::new(root.clone())),
            limits: Mutex::new(limits::LimitsReader::new()),
            frozen_now: None,
            timezone: chrono_tz::UTC,
            fixture: true,
            fixture_notifications: false,
            connection: limits::LimitsConnection::with_default_transport(Arc::new(
                FixtureCredentials::default(),
            )),
            last_port: std::sync::atomic::AtomicU16::new(49216),
            connection_generation: std::sync::atomic::AtomicU64::new(1),
            connection_gate: tokio::sync::Mutex::new(()),
            thread_busy: Default::default(),
            usage_busy: Default::default(),
            limits_busy: Default::default(),
        });
        refresh_connection(&state, false);
        assert_eq!(state.connection_generation.load(Ordering::Acquire), 2);
        assert!(!connection_operation_current(&state, 1));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn panel_height_is_bounded_to_native_panel_contract() {
        assert_eq!(panel_height(20.0), 80.0);
        assert_eq!(panel_height(320.0), 320.0);
        assert_eq!(panel_height(900.0), 560.0);
    }
    #[test]
    fn fixture_keyring_cannot_select_production_or_arbitrary_namespaces() {
        assert!(credential_store(false, Some("com.t3code.bar.tauri.limits")).is_err());
        assert!(credential_store(true, Some("com.t3code.bar.tauri.limits")).is_err());
        assert!(credential_store(true, Some("00000000-0000-0000-0000-000000000000")).is_err());
        let namespace = uuid::Uuid::new_v4().to_string();
        assert!(credential_store(false, Some(&namespace)).is_err());
        assert!(credential_store(true, Some(&namespace)).is_ok());
    }
    #[test]
    fn usage_period_mapping_matches_frontend_labels() {
        assert_eq!(usage_period("24h"), usage::Period::Day);
        assert_eq!(usage_period("7d"), usage::Period::Week);
        assert_eq!(usage_period("30d"), usage::Period::Month);
        assert_eq!(usage_period("90d"), usage::Period::Quarter);
        assert_eq!(usage_period("unexpected"), usage::Period::Day);
    }
}
