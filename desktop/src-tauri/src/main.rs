#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod credential_gates;
mod environment_metadata;

#[cfg(target_os = "linux")]
mod linux_appearance;
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

use bar_core::connection::{
    catalog::{EnvironmentMetadata, Route},
    cloud::CloudConfig,
    cloud_session::{CloudSessionController, LoginStatus},
    credentials::{
        forget_saved_access, CredentialStore as RemoteCredentialStore, MemoryCredentialStore,
    },
    relay::RelayClient,
    ssh::{SshConnectOptions, SshRouteError},
    ConnectionError, VerifiedEndpoint,
};
use bar_core::{
    environments::{
        AttemptStatus, EnvironmentHub, ScopedThread, Section, SourceAttempt, SourceStamp,
        StaleReason, UnavailableReason,
    },
    limits,
    remote::{RemoteConnection, RemoteReadError, SourceSupervisor},
    settings::{SettingKey, Settings, SettingsStore},
    threads, usage,
};
use bar_identity::{
    EnvironmentAccess, EnvironmentId, EnvironmentMachineKind, EnvironmentSelection,
    EnvironmentView, ThreadId, ThreadRef,
};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
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
#[cfg(target_os = "macos")]
static PANEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadItem {
    thread: ThreadRef,
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
    revision: u64,
    environments: Vec<EnvironmentView>,
    startup_error: Option<String>,
    cloud_status: String,
    cloud_error: Option<String>,
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
            revision: 0,
            environments: vec![],
            startup_error: None,
            cloud_status: "signedOut".into(),
            cloud_error: None,
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
    usage_window: Option<usage::Window>,
}
struct RuntimeState {
    config: threads::Configuration,
    hub: Mutex<EnvironmentHub<usage::UsageContribution, limits::LimitsContribution>>,
    snapshot: Mutex<AppSnapshot>,
    view: Mutex<ViewState>,
    sources: Mutex<HashMap<EnvironmentId, threads::Configuration>>,
    remote_routes: Mutex<HashMap<EnvironmentId, Vec<Route>>>,
    remote_pairing_codes: Mutex<HashMap<EnvironmentId, String>>,
    remote_supervisors: Mutex<HashMap<EnvironmentId, (Route, SourceSupervisor)>>,
    ssh_options: SshConnectOptions,
    remote_credentials: Arc<dyn RemoteCredentialStore>,
    credential_gates: credential_gates::CredentialGates,
    fixture_pairing_seeded: std::sync::atomic::AtomicBool,
    cloud: Option<CloudSessionController>,
    cloud_generation: std::sync::atomic::AtomicU64,
    favicons: favicons::Favicons,
    usage: Mutex<HashMap<EnvironmentId, usage::UsageReader>>,
    limits: Mutex<HashMap<EnvironmentId, limits::LimitsReader>>,
    frozen_now: Option<DateTime<Utc>>,
    timezone: Tz,
    fixture: bool,
    fixture_notifications: bool,
    connection: limits::LimitsConnection,
    descriptor_machine: Mutex<Option<EnvironmentMachineKind>>,
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
fn remote_credential_store(fixture: bool) -> Arc<dyn RemoteCredentialStore> {
    if fixture {
        Arc::new(MemoryCredentialStore::default())
    } else {
        Arc::new(bar_core::connection::credentials::KeyringStore::production())
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
fn environment_cache_path(settings: &Path) -> PathBuf {
    let mut path = settings.as_os_str().to_os_string();
    path.push(".environments-cache-v1");
    PathBuf::from(path)
}

fn iso(d: Option<DateTime<Utc>>) -> Option<String> {
    d.map(|v| v.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}
fn thread_item(
    t: threads::ThreadItem,
    environment_id: &EnvironmentId,
    state: &RuntimeState,
) -> Option<ThreadItem> {
    let project_favicon = if state.fixture {
        None
    } else {
        state.favicons.resolve(
            t.workspace_root.as_deref(),
            t.project_favicon_path.as_deref(),
        )
    };
    Some(ThreadItem {
        project_favicon,
        thread: ThreadRef {
            environment_id: environment_id.clone(),
            thread_id: ThreadId::new(&t.id).ok()?,
        },
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
    })
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
fn selected(selection: &EnvironmentSelection, environment: &EnvironmentId) -> bool {
    match selection {
        EnvironmentSelection::All => true,
        EnvironmentSelection::One(id) => id == environment,
    }
}

fn environment_access(routes: &[Route]) -> EnvironmentAccess {
    let direct = routes.iter().any(directly_pairable);
    let cloud = routes.iter().any(|route| matches!(route, Route::Relay));
    match (direct, cloud) {
        (true, true) => EnvironmentAccess::DirectOrCloud,
        (true, false) => EnvironmentAccess::Direct,
        (false, true) => EnvironmentAccess::Cloud,
        (false, false) => EnvironmentAccess::Unsupported,
    }
}

fn view_snapshot(state: &RuntimeState) -> Result<AppSnapshot, String> {
    let view = state.view.lock().map_err(|_| "view unavailable")?.clone();
    let hub = state.hub.lock().map_err(|_| "environments unavailable")?;
    let mut snapshot = state
        .snapshot
        .lock()
        .map_err(|_| "snapshot unavailable")?
        .clone();
    snapshot.cloud_status = match state.cloud.as_ref().and_then(|c| c.status().ok()) {
        Some(LoginStatus::SignedOut) => "signedOut",
        Some(LoginStatus::SigningIn) => "signingIn",
        Some(LoginStatus::SignedIn) => "signedIn",
        Some(LoginStatus::RecoveryRequired) => "recoveryRequired",
        None => "unavailable",
    }
    .into();
    let local_sources = state.sources.lock().map_err(|_| "sources unavailable")?;
    let local_id = local_environment(&state.config).ok();
    snapshot.environments = hub
        .sources()
        .values()
        .map(|source| EnvironmentView {
            id: source.metadata.id.clone(),
            label: source.metadata.label.clone(),
            is_local: local_id.as_ref() == Some(&source.metadata.id),
            machine: source.machine,
            access: environment_access(&source.metadata.routes),
            can_forget_access: !local_sources.contains_key(&source.metadata.id)
                && source.metadata.routes.iter().any(directly_pairable),
            status: if !source.present {
                "offline"
            } else if source.stamp.is_none()
                && !source.metadata.routes.is_empty()
                && source
                    .metadata
                    .routes
                    .iter()
                    .all(|route| matches!(route, Route::Ssh { target: None, .. }))
            {
                "unsupported"
            } else if source.attempt_status.is_some() {
                match source.attempt_status.as_ref().unwrap() {
                    AttemptStatus::Offline => "offline",
                    AttemptStatus::NeedsPairing => "needsAccess",
                    AttemptStatus::Unsupported => "unsupported",
                    AttemptStatus::PermissionDenied => "permissionDenied",
                }
            } else if source.stamp.is_none() {
                match source.snapshot.as_ref().map(|snapshot| &snapshot.threads) {
                    Some(Section::Unavailable(UnavailableReason::PermissionDenied)) => {
                        "permissionDenied"
                    }
                    Some(Section::Unavailable(UnavailableReason::UnsupportedRoute)) => {
                        "unsupported"
                    }
                    _ => "needsAccess",
                }
            } else if matches!(
                source.snapshot.as_ref().map(|snapshot| &snapshot.threads),
                Some(Section::Unavailable(UnavailableReason::PermissionDenied))
            ) {
                "permissionDenied"
            } else if matches!(
                source.snapshot.as_ref().map(|snapshot| &snapshot.threads),
                Some(Section::Unavailable(UnavailableReason::NeedsPairing))
            ) {
                "needsAccess"
            } else if matches!(
                source.snapshot.as_ref().map(|s| &s.threads),
                Some(Section::Ready { .. })
            ) && local_sources
                .get(&source.metadata.id)
                .is_none_or(|config| config.is_server_online())
            {
                "online"
            } else {
                "offline"
            }
            .into(),
        })
        .collect();
    let selected_views: Vec<_> = snapshot
        .environments
        .iter()
        .filter(|environment| selected(hub.selection(), &environment.id))
        .collect();
    let selected_online = selected_views
        .iter()
        .any(|environment| environment.status == "online");
    let selected_access = hub
        .selected_sources()
        .any(|source| !local_sources.contains_key(&source.metadata.id) && source.stamp.is_some());
    snapshot.online = selected_online;
    if !selected_views.is_empty() {
        snapshot.connection.paired = selected_access
            || (snapshot.connection.paired
                && local_environment(&state.config)
                    .ok()
                    .is_some_and(|id| selected(hub.selection(), &id)));
        snapshot.connection.connected = selected_online;
        if selected_online {
            snapshot.connection.status = "connected".into();
            snapshot.connection.message = "Connected to selected environments".into();
        } else {
            snapshot.connection.status = "offline".into();
            snapshot.connection.message = "Selected environments are offline or need access".into();
        }
    }
    if selected_online {
        snapshot.thread_error = None;
    }
    snapshot.threads = hub
        .selected_threads()
        .filter_map(|t| thread_item(t.item().clone(), &t.reference().environment_id, state))
        .collect();
    let selected_sources: Vec<_> = hub.selected_sources().collect();
    let selected: Vec<_> = selected_sources
        .iter()
        .filter_map(|source| source.snapshot.as_ref())
        .collect();
    let limits_values: Vec<_> = selected
        .iter()
        .filter_map(|s| match &s.limits {
            Section::Ready { value, .. } | Section::Stale { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect();
    snapshot.limits = (!limits_values.is_empty()).then(|| {
        report_limits(
            limits::merge_contributions(&limits_values, hub.selection()),
            now(state),
            selected_sources.iter().any(|s| {
                !matches!(
                    s.snapshot.as_ref().map(|snapshot| &snapshot.limits),
                    Some(Section::Ready { .. })
                )
            }),
        )
    });
    if let Some(window) = view
        .usage_window
        .as_ref()
        .filter(|w| w.period == usage_period(&view.period))
    {
        let query = usage::UsageQuery::for_window(window, None);
        let values: Vec<_> = selected
            .iter()
            .filter_map(|s| match &s.usage {
                Section::Ready { value, .. } | Section::Stale { value, .. }
                    if value.query() == &query =>
                {
                    Some(value.clone())
                }
                _ => None,
            })
            .collect();
        snapshot.usage = (!values.is_empty()).then(|| {
            report_usage(
                usage::merge_usage(
                    &values,
                    &query,
                    hub.selection(),
                    usage::USAGE_CONTRACT_VERSION,
                )
                .to_report(window),
                state.timezone,
            )
        });
        snapshot.usage_error = selected_sources.iter().any(|s| !matches!(s.snapshot.as_ref().map(|snapshot| &snapshot.usage), Some(Section::Ready { value, .. }) if value.query() == &query)).then(|| "Some environments could not refresh usage. Showing available data.".into());
    } else {
        snapshot.usage = None;
    }
    Ok(snapshot)
}
fn emit_snapshot(app: &tauri::AppHandle, state: &RuntimeState) {
    if let Ok(hub) = state.hub.lock() {
        let path = environment_cache_path(&settings_path(&state.config.base_dir));
        if let Err(error) = hub.save_cache(&path) {
            eprintln!("Cannot save environment cache: {error}");
        }
    }
    if let Ok(snapshot) = view_snapshot(state) {
        let _ = app.emit("bar-snapshot", snapshot);
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
    let metadata_state = state.clone();
    tauri::async_runtime::spawn(async move {
        let machine = match local_environment(&metadata_state.config) {
            Ok(id) if port != 0 => environment_metadata::descriptor_machine(port, &id).await,
            _ => None,
        };
        if connection_operation_current(&metadata_state, generation) {
            if let Ok(mut current) = metadata_state.descriptor_machine.lock() {
                *current = machine;
            }
        }
    });
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
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureEnvironment {
    id: EnvironmentId,
    label: String,
    #[serde(default)]
    state_dir: Option<PathBuf>,
    #[serde(default)]
    routes: Vec<Route>,
    #[serde(default)]
    pairing_code: Option<String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum FixtureManifest {
    List(Vec<FixtureEnvironment>),
    Document {
        environments: Vec<FixtureEnvironment>,
    },
}
impl FixtureManifest {
    fn into_entries(self) -> Vec<FixtureEnvironment> {
        match self {
            Self::List(entries) => entries,
            Self::Document { environments } => environments,
        }
    }
}

fn discover_sources(state: &RuntimeState) -> Result<(), String> {
    use bar_core::environments::DiscoveryState;
    let local = local_environment(&state.config)?;
    let mut primary = EnvironmentMetadata {
        id: local.clone(),
        label: "Local".into(),
        enabled: true,
        routes: Vec::new(),
    };
    if let Ok(port) = state.config.server_port() {
        primary.routes.push(Route::Primary {
            http_base_url: format!("http://127.0.0.1:{port}"),
        });
    }
    let mut paths = HashMap::new();
    paths.insert(local.clone(), state.config.clone());
    let entries = if state.fixture {
        if let Some(path) = fixture_or_env("T3_BAR_FIXTURE_ENVIRONMENTS") {
            let entries = serde_json::from_slice::<FixtureManifest>(
                &std::fs::read(path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?
            .into_entries();
            let mut metadata = vec![primary.clone()];
            let seed_codes = !state
                .fixture_pairing_seeded
                .swap(true, std::sync::atomic::Ordering::AcqRel);
            for entry in entries {
                if let Some(state_dir) = entry.state_dir {
                    if !state_dir.is_absolute() {
                        return Err("Fixture state path must be absolute".into());
                    }
                    let config = threads::Configuration {
                        base_dir: state_dir,
                    };
                    if local_environment(&config)? != entry.id {
                        return Err("Fixture environment identity mismatch".into());
                    }
                    paths.insert(entry.id.clone(), config);
                }
                if let Some(pairing_code) = entry.pairing_code.filter(|_| seed_codes) {
                    state
                        .remote_pairing_codes
                        .lock()
                        .map_err(|_| "remote pairing state unavailable")?
                        .insert(entry.id.clone(), pairing_code);
                }
                metadata.push(EnvironmentMetadata {
                    id: entry.id,
                    label: entry.label,
                    enabled: true,
                    routes: entry.routes,
                });
            }
            Ok(metadata)
        } else {
            Ok(vec![primary.clone()])
        }
    } else {
        bar_core::connection::catalog::discover(
            &state.config.base_dir.join("connection-catalog.json"),
            primary.clone(),
        )
        .map_err(|_| ())
    };
    let mut hub = state.hub.lock().map_err(|_| "environments unavailable")?;
    if matches!(hub.discovery_state(), DiscoveryState::Pending)
        || (entries.is_err() && !hub.sources().contains_key(&local))
    {
        let mut known: Vec<_> = hub
            .sources()
            .values()
            .filter(|source| source.present)
            .map(|source| source.metadata.clone())
            .collect();
        known.push(primary);
        hub.apply_discovery(Ok(known));
    }
    hub.apply_discovery(entries);
    hub.set_machine(
        &local,
        Some(environment_metadata::local_machine(
            &state.config.base_dir,
            state.descriptor_machine.lock().ok().and_then(|m| *m),
        )),
        None,
    );
    for id in paths.keys() {
        if hub
            .sources()
            .get(id)
            .is_some_and(|source| source.present && source.stamp.is_none())
        {
            hub.begin_next_source(id, "local");
        }
    }
    let remote_entries: Vec<_> = hub
        .sources()
        .values()
        .filter(|source| !paths.contains_key(&source.metadata.id))
        .map(|source| (source.metadata.id.clone(), source.metadata.routes.clone()))
        .collect();
    drop(hub);
    let mut remote_routes = state
        .remote_routes
        .lock()
        .map_err(|_| "remote routes unavailable")?;
    remote_routes.clear();
    for (environment, routes) in remote_entries {
        if !routes.is_empty() {
            remote_routes.insert(environment, routes);
        }
    }
    *state.sources.lock().map_err(|_| "sources unavailable")? = paths;
    Ok(())
}

fn remote_route_url(route: &Route) -> Option<&str> {
    match route {
        Route::Primary { http_base_url }
        | Route::Bearer {
            http_base_url: Some(http_base_url),
            ..
        } if !http_base_url.is_empty() => Some(http_base_url.as_str()),
        _ => None,
    }
}

/// Routes where a user-provided Bar pairing code can create Bar's own grant.
fn directly_pairable(route: &Route) -> bool {
    remote_route_url(route).is_some()
        || matches!(
            route,
            Route::Ssh {
                target: Some(_),
                ..
            }
        )
}

fn map_error(error: ConnectionError) -> RemoteReadError {
    match error {
        ConnectionError::PermissionDenied | ConnectionError::PairingRejected => {
            RemoteReadError::PermissionDenied {
                threads: true,
                usage: true,
                limits: true,
            }
        }
        ConnectionError::RelayCredentialUnsupported => RemoteReadError::UnsupportedRoute,
        ConnectionError::Transport => RemoteReadError::Offline,
        _ => RemoteReadError::Protocol,
    }
}

fn map_ssh_error(error: SshRouteError) -> RemoteReadError {
    match error {
        SshRouteError::InvalidTarget => RemoteReadError::UnsupportedRoute,
        SshRouteError::MalformedMetadata => RemoteReadError::Protocol,
        _ => RemoteReadError::Offline,
    }
}

/// A locally expired grant needs new access but is not a server denial, so a
/// later route may still serve the environment.
fn map_load_error(error: ConnectionError) -> RemoteReadError {
    match error {
        ConnectionError::GrantExpired => RemoteReadError::NeedsPairing,
        other => map_error(other),
    }
}

/// Bar's own grant for the verified endpoint. A pairing code the user supplied
/// replaces any saved grant and is tried first, so a failed attempt keeps the
/// old grant. Otherwise the saved grant is used. The attempt is checked inside
/// the environment's credential gate, so access forgotten while this call
/// waited is never read or recreated. Never reads upstream credentials.
fn load_or_pair_grant(
    state: &RuntimeState,
    runtime: &tokio::runtime::Runtime,
    environment: &EnvironmentId,
    endpoint: &VerifiedEndpoint,
    attempt: &SourceAttempt,
) -> Result<bar_core::connection::Grant, RemoteReadError> {
    state.credential_gates.serialize(environment, || {
        if !state
            .hub
            .lock()
            .map_err(|_| RemoteReadError::Offline)?
            .attempt_current(attempt)
        {
            return Err(RemoteReadError::Offline);
        }
        let code = state
            .remote_pairing_codes
            .lock()
            .map_err(|_| RemoteReadError::Offline)?
            .get(environment)
            .cloned();
        if let Some(code) = code {
            let grant = runtime
                .block_on(endpoint.pair_and_save(&code, state.remote_credentials.as_ref()))
                .map_err(map_error)?;
            if let Ok(mut codes) = state.remote_pairing_codes.lock() {
                codes.remove(environment);
            }
            return Ok(grant);
        }
        endpoint
            .load_saved_grant(state.remote_credentials.as_ref())
            .map_err(map_load_error)?
            .ok_or(RemoteReadError::NeedsPairing)
    })
}

/// Take back the stored SSH session only while it is the current source's
/// session on the same route, so a poll reuses its tunnel instead of opening
/// another. Anything else is dropped here, which reaps its tunnel.
fn take_reusable_ssh(
    state: &RuntimeState,
    environment: &EnvironmentId,
    route: &Route,
    attempt: &SourceAttempt,
) -> Option<SourceSupervisor> {
    let (saved_route, mut supervisor) =
        state.remote_supervisors.lock().ok()?.remove(environment)?;
    let current = {
        let hub = state.hub.lock().ok()?;
        if !hub.attempt_current(attempt) {
            return None;
        }
        hub.sources().get(environment)?.stamp.clone()?
    };
    let current = bar_core::remote::SourceStamp {
        environment: current.environment,
        generation: current.generation,
        principal: current.principal,
    };
    (saved_route == *route && supervisor.stamp() == &current && supervisor.usable())
        .then_some(supervisor)
}

#[derive(Clone)]
enum RemoteReadQuery {
    Snapshot(Option<usage::UsageQuery>),
    Preview(String),
}

fn read_remote_source(
    state: &RuntimeState,
    environment: &EnvironmentId,
    routes: &[Route],
    query: RemoteReadQuery,
    attempt: &mut SourceAttempt,
) -> Result<bar_core::remote::RemoteReadResult, RemoteReadError> {
    let mut last = RemoteReadError::UnsupportedRoute;
    for route in routes {
        match read_remote_route(state, environment, route, query.clone(), attempt) {
            Ok(result) => return Ok(result),
            Err(error) if retry_remote_route(&error) => {
                if !matches!(last, RemoteReadError::NeedsPairing)
                    || matches!(error, RemoteReadError::NeedsPairing)
                {
                    last = error;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(last)
}

fn retry_remote_route(error: &RemoteReadError) -> bool {
    matches!(
        error,
        RemoteReadError::NeedsPairing
            | RemoteReadError::Offline
            | RemoteReadError::UnsupportedRoute
            | RemoteReadError::Rpc(
                bar_core::connection::rpc::RpcError::Transport
                    | bar_core::connection::rpc::RpcError::Closed
            )
            | RemoteReadError::Rpc(bar_core::connection::rpc::RpcError::Connection(
                bar_core::connection::ConnectionError::Transport
            ))
    )
}

fn read_remote_route(
    state: &RuntimeState,
    environment: &EnvironmentId,
    route: &Route,
    query: RemoteReadQuery,
    attempt: &mut SourceAttempt,
) -> Result<bar_core::remote::RemoteReadResult, RemoteReadError> {
    static REMOTE_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> =
        std::sync::OnceLock::new();
    let runtime =
        REMOTE_RUNTIME.get_or_init(|| tokio::runtime::Runtime::new().expect("remote runtime"));
    let pairing_requested = state
        .remote_pairing_codes
        .lock()
        .map_err(|_| RemoteReadError::Offline)?
        .contains_key(environment);
    let reused = match route {
        Route::Ssh { .. } if !pairing_requested => {
            take_reusable_ssh(state, environment, route, attempt)
        }
        _ => None,
    };
    let (connection, principal) = if let Some(reused) = &reused {
        (None, reused.stamp().principal.clone())
    } else if matches!(route, Route::Relay) {
        let controller = state.cloud.as_ref().ok_or(RemoteReadError::Protocol)?;
        let (cloud, lease) = controller
            .load_session()
            .map_err(map_error)?
            .ok_or(RemoteReadError::NeedsPairing)?;
        let principal = format!("cloud:{}", cloud.cache_key());
        let relay = RelayClient::production().map_err(map_error)?;
        let (connection, _) = runtime
            .block_on(relay.connect_refreshing(environment, controller.config(), cloud, &lease))
            .map_err(map_error)?;
        (Some(RemoteConnection::Relay(connection)), principal)
    } else if let Route::Ssh { target, .. } = route {
        let target = target.clone().ok_or(RemoteReadError::UnsupportedRoute)?;
        let tunnel = runtime
            .block_on(bar_core::connection::ssh::connect_async(
                target,
                state.ssh_options.clone(),
            ))
            .map_err(map_ssh_error)?;
        let endpoint = runtime
            .block_on(tunnel.verified_endpoint(environment))
            .map_err(map_error)?;
        let grant = load_or_pair_grant(state, runtime, environment, &endpoint, attempt)?;
        let principal = grant.principal_identity().as_key();
        (
            Some(RemoteConnection::Ssh {
                endpoint,
                grant,
                tunnel,
            }),
            principal,
        )
    } else {
        let url = remote_route_url(route).ok_or(RemoteReadError::UnsupportedRoute)?;
        let endpoint = runtime
            .block_on(VerifiedEndpoint::discover(url, environment))
            .map_err(map_error)?;
        let grant = load_or_pair_grant(state, runtime, environment, &endpoint, attempt)?;
        let principal = grant.principal_identity().as_key();
        (
            Some(RemoteConnection::Direct { endpoint, grant }),
            principal,
        )
    };
    let stamp = {
        let mut hub = state.hub.lock().map_err(|_| RemoteReadError::Offline)?;
        if !hub.attempt_current(attempt) {
            return Err(RemoteReadError::Offline);
        }
        if let Some(stamp) = hub
            .sources()
            .get(environment)
            .and_then(|source| source.stamp.clone())
            .filter(|stamp| stamp.principal == principal)
        {
            stamp
        } else {
            hub.begin_attempt_source(attempt, principal.clone())
                .ok_or(RemoteReadError::Offline)?
        }
    };
    let label = state
        .hub
        .lock()
        .ok()
        .and_then(|hub| {
            hub.sources()
                .get(environment)
                .map(|source| source.metadata.label.clone())
        })
        .unwrap_or_else(|| environment.to_string());
    let remote_stamp = bar_core::remote::SourceStamp {
        environment: stamp.environment.clone(),
        generation: stamp.generation,
        principal: stamp.principal.clone(),
    };
    let mut supervisor = match reused {
        Some(supervisor) => supervisor,
        None => state
            .remote_supervisors
            .lock()
            .map_err(|_| RemoteReadError::Offline)?
            .remove(environment)
            .filter(|(saved_route, supervisor)| {
                !matches!(route, Route::Relay | Route::Ssh { .. })
                    && saved_route == route
                    && supervisor.stamp() == &remote_stamp
            })
            .and_then(|(_, mut supervisor)| supervisor.usable().then_some(supervisor))
            .unwrap_or_else(|| {
                SourceSupervisor::new(
                    connection.expect("a fresh connection exists when no session is reused"),
                    remote_stamp.clone(),
                    label,
                )
            }),
    };
    let result = runtime.block_on(async {
        match &query {
            RemoteReadQuery::Snapshot(usage) => supervisor.read(usage.as_ref()).await,
            RemoteReadQuery::Preview(thread) => supervisor.read_preview(thread).await,
        }
    });
    {
        let mut hub = state.hub.lock().map_err(|_| RemoteReadError::Offline)?;
        if !hub.attempt_current(attempt) {
            return Err(RemoteReadError::Offline);
        }
        if result.is_ok() {
            hub.complete_attempt(attempt);
            // Stored while the attempt is known current, so a concurrent
            // forget cannot be followed by a stale session reappearing.
            if !matches!(route, Route::Relay) {
                if let Ok(mut supervisors) = state.remote_supervisors.lock() {
                    supervisors.insert(environment.clone(), (route.clone(), supervisor));
                }
            }
        }
    }
    result
}

fn apply_remote_error(
    state: &RuntimeState,
    attempt: &SourceAttempt,
    error: RemoteReadError,
) -> Option<String> {
    let status = match &error {
        RemoteReadError::NeedsPairing => Some(AttemptStatus::NeedsPairing),
        RemoteReadError::UnsupportedRoute => Some(AttemptStatus::Unsupported),
        RemoteReadError::PermissionDenied { threads: false, .. } => None,
        RemoteReadError::PermissionDenied { .. }
        | RemoteReadError::Rpc(bar_core::connection::rpc::RpcError::Connection(
            bar_core::connection::ConnectionError::PermissionDenied,
        )) => Some(AttemptStatus::PermissionDenied),
        _ => Some(AttemptStatus::Offline),
    };
    let mut hub = state.hub.lock().ok()?;
    let current = match status {
        Some(status) => hub.fail_attempt(attempt, status),
        None => hub.complete_attempt(attempt),
    };
    if !current {
        return None;
    }
    let stamp = hub
        .sources()
        .get(&attempt.environment)
        .and_then(|source| source.stamp.clone());
    match error {
        RemoteReadError::NeedsPairing => {
            if let Some(stamp) = stamp {
                hub.unavailable_sections(&stamp, true, true, true, UnavailableReason::NeedsPairing);
            }
            None
        }
        RemoteReadError::UnsupportedRoute => {
            if let Some(stamp) = stamp {
                hub.unavailable_sections(
                    &stamp,
                    true,
                    true,
                    true,
                    UnavailableReason::UnsupportedRoute,
                );
            }
            None
        }
        RemoteReadError::PermissionDenied {
            threads,
            usage,
            limits,
        } => {
            if let Some(stamp) = stamp {
                hub.unavailable_sections(
                    &stamp,
                    threads,
                    usage,
                    limits,
                    UnavailableReason::PermissionDenied,
                );
            }
            None
        }
        RemoteReadError::Rpc(bar_core::connection::rpc::RpcError::Connection(
            bar_core::connection::ConnectionError::PermissionDenied,
        )) => {
            if let Some(stamp) = stamp {
                hub.unavailable_sections(
                    &stamp,
                    true,
                    true,
                    true,
                    UnavailableReason::PermissionDenied,
                );
            }
            None
        }
        RemoteReadError::Offline | RemoteReadError::Rpc(_) | RemoteReadError::Protocol => {
            if let Some(stamp) = stamp {
                hub.offline(&stamp);
            }
            Some("Remote environment is offline. Showing the last available data.".into())
        }
    }
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
        let discovery_error = discover_sources(&state).err();
        let paths = state.sources.lock().map(|s| s.clone()).unwrap_or_default();
        let remote_routes = state
            .remote_routes
            .lock()
            .map(|routes| routes.clone())
            .unwrap_or_default();
        let settings = state
            .hub
            .lock()
            .ok()
            .map(|h| h.settings().clone())
            .unwrap_or_default();
        let mut alerts = Vec::new();
        let mut online = false;
        let mut error = discovery_error;
        for (environment, config) in paths {
            match threads::read_threads(&config, now(&state)) {
                Ok(result) => {
                    if config.base_dir == state.config.base_dir {
                        online = result.is_server_online;
                    }
                    if let Ok(mut hub) = state.hub.lock() {
                        let stamp = hub
                            .sources()
                            .get(&environment)
                            .and_then(|s| s.stamp.clone());
                        if let Some(stamp) = stamp {
                            let rows: Vec<_> = result
                                .threads
                                .into_iter()
                                .filter_map(|item| ScopedThread::new(&environment, item))
                                .collect();
                            if let Some(new_alerts) = hub.apply_threads(
                                &stamp,
                                rows,
                                now(&state).timestamp(),
                                settings.notifications_muted
                                    || !state.connection.snapshot().has_credential
                                    || (state.fixture && !state.fixture_notifications),
                            ) {
                                alerts.extend(
                                    new_alerts
                                        .into_iter()
                                        .map(|alert| (environment.clone(), alert)),
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    error = Some(e.to_string());
                    if let Ok(mut hub) = state.hub.lock() {
                        if let Some(stamp) = hub
                            .sources()
                            .get(&environment)
                            .and_then(|s| s.stamp.clone())
                        {
                            hub.offline(&stamp);
                        }
                    }
                }
            }
        }
        for (environment, route) in remote_routes {
            let query = state.view.lock().ok().and_then(|view| {
                view.usage_window
                    .as_ref()
                    .filter(|window| window.period == usage_period(&view.period))
                    .map(|window| usage::UsageQuery::for_window(window, None))
            });
            let operation_query = query.clone();
            let Some(mut attempt) = state
                .hub
                .lock()
                .ok()
                .and_then(|mut hub| hub.begin_attempt(&environment))
            else {
                continue;
            };
            match read_remote_source(&state, &environment, &route, RemoteReadQuery::Snapshot(query), &mut attempt) {
                Ok(result) => {
                    let stamp = SourceStamp {
                        environment: result.stamp.environment.clone(),
                        generation: result.stamp.generation,
                        principal: result.stamp.principal.clone(),
                    };
                    let current_view = state.view.lock().ok();
                    let current_query = current_view.as_ref().and_then(|view| {
                        view.usage_window
                            .as_ref()
                            .filter(|window| window.period == usage_period(&view.period))
                            .map(|window| usage::UsageQuery::for_window(window, None))
                    });
                    if let Ok(mut hub) = state.hub.lock() {
                        if !hub.attempt_current(&attempt) {
                            continue;
                        }
                        if hub
                            .sources()
                            .get(&environment)
                            .and_then(|source| source.stamp.as_ref())
                            == Some(&stamp)
                        {
                            hub.set_machine(
                                &environment,
                                result.environment_icon_override,
                                result.machine,
                            );
                        }
                        let muted = hub.settings().notifications_muted
                            || (state.fixture && !state.fixture_notifications);
                        let rows: Vec<_> = result
                            .threads
                            .into_iter()
                            .filter_map(|item| ScopedThread::new(&environment, item))
                            .collect();
                        if let Some(new_alerts) =
                            hub.apply_threads(&stamp, rows, now(&state).timestamp(), muted)
                        {
                            alerts.extend(
                                new_alerts
                                    .into_iter()
                                    .map(|alert| (environment.clone(), alert)),
                            );
                        }
                        if let Some(contribution) = result.usage {
                            if let Some(expected) = operation_query
                                .as_ref()
                                .filter(|query| Some(*query) == current_query.as_ref())
                            {
                                apply_usage_contribution(
                                    &mut hub,
                                    &stamp,
                                    expected,
                                    contribution,
                                    now(&state).timestamp(),
                                );
                            }
                        } else if result.usage_permission_denied
                            && operation_query.is_some()
                            && operation_query == current_query
                        {
                            hub.unavailable_sections(
                                &stamp,
                                false,
                                true,
                                false,
                                UnavailableReason::PermissionDenied,
                            );
                        }
                        if let Some(contribution) = result.limits {
                            hub.update_sections(&stamp, |_, limits| {
                                *limits = Section::Ready {
                                    value: contribution,
                                    observed_at: now(&state).timestamp(),
                                }
                            });
                        } else if result.limits_permission_denied {
                            hub.unavailable_sections(
                                &stamp,
                                false,
                                false,
                                true,
                                UnavailableReason::PermissionDenied,
                            );
                        }
                    }
                }
                Err(remote_error) => {
                    let current_view = state.view.lock().ok();
                    let current_query = current_view.as_ref().and_then(|view| {
                        view.usage_window
                            .as_ref()
                            .filter(|window| window.period == usage_period(&view.period))
                            .map(|window| usage::UsageQuery::for_window(window, None))
                    });
                    if current_query == operation_query {
                        if let Some(message) = apply_remote_error(&state, &attempt, remote_error) {
                            error = Some(message);
                        }
                    }
                }
            }
        }
        let rows = state
            .hub
            .lock()
            .map(|hub| {
                hub.selected_threads()
                    .map(|row| row.item().clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let selected_online = view_snapshot(&state)
            .map(|snapshot| snapshot.online)
            .unwrap_or(false);
        tray_status::update(&app, &threads::ThreadSummary::new(&rows, selected_online));
        if let Ok(mut snap) = state.snapshot.lock() {
            snap.online = online;
            snap.thread_error = error;
            snap.now = now(&state).to_rfc3339();
            snap.loading = false;
            refresh_connection(&state, online);
            snap.connection = connection_for(
                &state,
                online,
                !rows.is_empty() || state.config.base_dir.join("statev2.sqlite").exists(),
            );
        }
        for (environment, alert) in alerts {
            if !state
                .hub
                .lock()
                .map(|hub| {
                    selected(hub.selection(), &environment) && !hub.settings().notifications_muted
                })
                .unwrap_or(false)
            {
                continue;
            }
            use tauri_plugin_notification::NotificationExt;
            let notification = app
                .notification()
                .builder()
                .title(alert.title())
                .body(&alert.body)
                .extra("environmentId", environment.to_string())
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
        let paths = state.sources.lock().map(|s| s.clone()).unwrap_or_default();
        for (id, config) in paths {
            let stamp = state
                .hub
                .lock()
                .ok()
                .and_then(|h| h.sources().get(&id).and_then(|s| s.stamp.clone()));
            let Some(stamp) = stamp else {
                continue;
            };
            if let Ok(mut readers) = state.limits.lock() {
                let reader = readers.entry(id.clone()).or_default();
                if !read_cache && config.base_dir != state.config.base_dir {
                    continue;
                }
                let result = if read_cache {
                    reader.read(&config.base_dir, now(&state)).map(|_| ())
                } else {
                    Ok(())
                };
                let value = if config.base_dir == state.config.base_dir {
                    reader
                        .live_contribution(&state.connection, id.clone())
                        .unwrap_or_else(|| reader.contribution(id.clone()))
                } else {
                    reader.contribution(id.clone())
                };
                if !read_cache && state.connection.snapshot().status != limits::LiveStatus::Live {
                    continue;
                }
                if let Ok(mut hub) = state.hub.lock() {
                    hub.update_sections(&stamp, |_, section| match result {
                        Ok(()) => {
                            *section = Section::Ready {
                                value,
                                observed_at: now(&state).timestamp(),
                            }
                        }
                        Err(_) => {
                            section.failed(StaleReason::ReadFailed, UnavailableReason::Missing)
                        }
                    });
                }
            }
        }
        state
            .limits_busy
            .store(false, std::sync::atomic::Ordering::Release);
        emit_snapshot(&app, &state);
    });
}
fn apply_usage_contribution(
    hub: &mut EnvironmentHub<usage::UsageContribution, limits::LimitsContribution>,
    stamp: &SourceStamp,
    expected: &usage::UsageQuery,
    contribution: usage::UsageContribution,
    observed_at: i64,
) -> bool {
    if contribution.environment_id() != &stamp.environment || contribution.query() != expected {
        return false;
    }
    hub.update_sections(stamp, |section, _| {
        *section = Section::Ready {
            value: contribution,
            observed_at,
        }
    })
}
fn refresh_visible(app: &tauri::AppHandle, state: &SharedState) {
    if !VISIBLE.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let view = state.view.lock().map(|v| v.clone()).unwrap_or_default();
    if view.screen != "usage"
        || view.metric == "limits"
        || state
            .usage_busy
            .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    let app = app.clone();
    let state = state.clone();
    thread::spawn(move || {
        let at = now(&state);
        let window = usage::Window::new(usage_period(&view.period), at, state.timezone);
        let paths = state.sources.lock().map(|s| s.clone()).unwrap_or_default();
        for (id, config) in paths {
            let source = state.hub.lock().ok().and_then(|h| {
                h.sources().get(&id).and_then(|s| {
                    s.stamp
                        .clone()
                        .map(|stamp| (stamp, s.metadata.label.clone()))
                })
            });
            let Some((stamp, label)) = source else {
                continue;
            };
            let result = state
                .usage
                .lock()
                .map_err(|_| usage::UsageError::Unreadable)
                .and_then(|mut readers| {
                    let reader = readers.entry(id.clone()).or_insert_with(|| {
                        let mut reader = usage::UsageReader::new(config.base_dir.clone());
                        if !state.fixture && config.base_dir == state.config.base_dir {
                            if let Some(home) = fixture_or_env("HOME") {
                                reader.set_local_roots(usage::LocalRoots::discover(
                                    &config.base_dir,
                                    PathBuf::from(home),
                                ));
                            }
                        }
                        reader
                    });
                    reader.read_contribution(
                        &window,
                        None,
                        &usage::LocalProvenance {
                            base_dir: config.base_dir.clone(),
                            ..Default::default()
                        },
                        id.clone(),
                        label,
                        at,
                    )
                });
            if let Ok(mut current) = state.view.lock() {
                if current.generation != view.generation
                    || current.screen != "usage"
                    || current.metric == "limits"
                    || !VISIBLE.load(std::sync::atomic::Ordering::Acquire)
                {
                    continue;
                }
                if let Ok(mut hub) = state.hub.lock() {
                    let applied = match result {
                        Ok(value) => {
                            let incomplete = !value.unknown_sources.is_empty();
                            let applied = apply_usage_contribution(
                                &mut hub,
                                &stamp,
                                &usage::UsageQuery::for_window(&window, None),
                                value.contribution,
                                at.timestamp(),
                            );
                            if applied && incomplete {
                                hub.update_sections(&stamp, |section, _| {
                                    section
                                        .failed(StaleReason::ReadFailed, UnavailableReason::Missing)
                                });
                            }
                            applied
                        }
                        Err(_) => hub.update_sections(&stamp, |section, _| {
                            section.failed(StaleReason::ReadFailed, UnavailableReason::Missing)
                        }),
                    };
                    if applied {
                        current.usage_window = Some(window.clone());
                    }
                }
            }
        }
        let remote_routes = state
            .remote_routes
            .lock()
            .map(|routes| routes.clone())
            .unwrap_or_default();
        let query = usage::UsageQuery::for_window(&window, None);
        for (environment, route) in remote_routes {
            let Some(mut attempt) = state
                .hub
                .lock()
                .ok()
                .and_then(|mut hub| hub.begin_attempt(&environment))
            else {
                continue;
            };
            match read_remote_source(
                &state,
                &environment,
                &route,
                RemoteReadQuery::Snapshot(Some(query.clone())),
                &mut attempt,
            ) {
                Ok(result) => {
                    let stamp = SourceStamp {
                        environment: result.stamp.environment.clone(),
                        generation: result.stamp.generation,
                        principal: result.stamp.principal.clone(),
                    };
                    if let Ok(mut current) = state.view.lock() {
                        if current.generation != view.generation
                            || current.screen != "usage"
                            || current.metric == "limits"
                            || !VISIBLE.load(std::sync::atomic::Ordering::Acquire)
                        {
                            continue;
                        }
                        if let Ok(mut hub) = state.hub.lock() {
                            if !hub.attempt_current(&attempt) {
                                continue;
                            }
                            if let Some(value) = result.usage {
                                apply_usage_contribution(
                                    &mut hub,
                                    &stamp,
                                    &query,
                                    value,
                                    at.timestamp(),
                                );
                                current.usage_window = Some(window.clone());
                            } else if result.usage_permission_denied {
                                hub.unavailable_sections(
                                    &stamp,
                                    false,
                                    true,
                                    false,
                                    UnavailableReason::PermissionDenied,
                                );
                            }
                        }
                    }
                }
                Err(error) => {
                    let current = state
                        .view
                        .lock()
                        .ok()
                        .is_some_and(|current| current.generation == view.generation);
                    if current {
                        let _ = apply_remote_error(&state, &attempt, error);
                    }
                }
            }
        }
        if let Ok(mut snap) = state.snapshot.lock() {
            snap.usage_period = view.period.clone();
            snap.usage_loading = false;
        }
        state
            .usage_busy
            .store(false, std::sync::atomic::Ordering::Release);
        emit_snapshot(&app, &state);
    });
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
    view_snapshot(&state)
}
fn cloud_error(state: &RuntimeState, message: Option<&str>) {
    if let Ok(mut snapshot) = state.snapshot.lock() {
        snapshot.cloud_error = message.map(str::to_owned);
    }
}

fn invalidate_cloud_sources(state: &RuntimeState) {
    if let Ok(mut supervisors) = state.remote_supervisors.lock() {
        supervisors.retain(|_, (route, _)| !matches!(route, Route::Relay));
    }
    if let Ok(mut hub) = state.hub.lock() {
        let ids: Vec<_> = hub
            .sources()
            .values()
            .filter(|source| {
                source
                    .metadata
                    .routes
                    .iter()
                    .any(|route| matches!(route, Route::Relay))
            })
            .map(|source| source.metadata.id.clone())
            .collect();
        for id in ids {
            let stamp = hub
                .sources()
                .get(&id)
                .and_then(|source| source.stamp.clone())
                .filter(|stamp| stamp.principal.starts_with("cloud:"));
            if let Some(stamp) = stamp {
                hub.revoke(&stamp);
            }
            if let Some(attempt) = hub.begin_attempt(&id) {
                hub.fail_attempt(&attempt, AttemptStatus::NeedsPairing);
            }
        }
    }
}

#[tauri::command]
async fn begin_cloud_login(
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let controller = state
        .cloud
        .as_ref()
        .ok_or("T3 Connect is unavailable")?
        .clone();
    let generation = state
        .cloud_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
        .wrapping_add(1);
    cloud_error(&state, None);
    let result = controller.begin_login().await;
    emit_snapshot(&app, &state);
    let (url, pending) = match result {
        Ok(value) => value,
        Err(_) => {
            if state
                .cloud_generation
                .load(std::sync::atomic::Ordering::Acquire)
                == generation
            {
                cloud_error(&state, Some("Could not start T3 Connect sign in."));
                emit_snapshot(&app, &state);
            }
            return Err("Could not start T3 Connect sign in".into());
        }
    };
    if app
        .opener()
        .open_url(url.to_string(), None::<String>)
        .is_err()
    {
        drop(pending);
        cloud_error(&state, Some("Could not open the sign in page."));
        emit_snapshot(&app, &state);
        return Err("Could not open the sign in page".into());
    }
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let result = pending.complete().await;
        if result.is_err()
            && state
                .cloud_generation
                .load(std::sync::atomic::Ordering::Acquire)
                == generation
        {
            cloud_error(&state, Some("T3 Connect sign in did not complete."));
        }
        if result.is_ok() {
            cloud_error(&state, None);
        }
        emit_snapshot(&app, &state);
        if result.is_ok() {
            refresh_threads(&app, &state);
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_cloud_login(state: State<'_, SharedState>, app: tauri::AppHandle) -> Result<(), String> {
    state
        .cloud_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    let result = state
        .cloud
        .as_ref()
        .ok_or("T3 Connect is unavailable")?
        .cancel_login()
        .map_err(|_| "Could not cancel sign in".to_string());
    invalidate_cloud_sources(&state);
    cloud_error(&state, None);
    emit_snapshot(&app, &state);
    result
}

#[tauri::command]
fn disconnect_cloud(state: State<'_, SharedState>, app: tauri::AppHandle) -> Result<(), String> {
    state
        .cloud_generation
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    let result = state
        .cloud
        .as_ref()
        .ok_or("T3 Connect is unavailable")?
        .disconnect()
        .map_err(|_| {
            "Could not remove all T3 Connect credentials. Some may remain. Try signing out again."
                .to_string()
        });
    invalidate_cloud_sources(&state);
    cloud_error(&state, result.as_ref().err().map(String::as_str));
    emit_snapshot(&app, &state);
    result
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
    let mut hub = state.hub.lock().map_err(|_| "settings unavailable")?;
    let mut snapshot = state.snapshot.lock().map_err(|_| "snapshot unavailable")?;
    hub.set_setting(key, value).map_err(|e| e.to_string())?;
    snapshot.settings = hub.settings().clone();
    drop(snapshot);
    drop(hub);
    let _ = app.emit("bar-settings", ());
    emit_snapshot(&app, &state);
    Ok(())
}
#[tauri::command]
fn set_environment_selection(
    selection: EnvironmentSelection,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut hub = state.hub.lock().map_err(|_| "settings unavailable")?;
    let mut snapshot = state.snapshot.lock().map_err(|_| "snapshot unavailable")?;
    snapshot.revision = hub.set_selection(selection).map_err(|e| e.to_string())?;
    snapshot.settings = hub.settings().clone();
    drop(snapshot);
    drop(hub);
    emit_snapshot(&app, &state);
    Ok(())
}
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}
/// The hub snapshot is the only authority for which threads exist. Callers must
/// not hold the snapshot lock, which the hub lock precedes.
fn ensure_listed(state: &RuntimeState, thread: &ThreadRef) -> Result<(), String> {
    if state
        .hub
        .lock()
        .map_err(|_| "environments unavailable")?
        .lists_thread(thread)
    {
        Ok(())
    } else {
        Err("Unknown thread".into())
    }
}

fn local_source(
    state: &RuntimeState,
    environment: &EnvironmentId,
) -> Result<Option<threads::Configuration>, String> {
    Ok(state
        .sources
        .lock()
        .map_err(|_| "sources unavailable")?
        .get(environment)
        .cloned())
}

fn encode_path_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn runtime_thread_url(
    state: &RuntimeState,
    thread: &ThreadRef,
    browser: bool,
) -> Result<String, String> {
    ensure_listed(state, thread)?;
    if let Some(config) = local_source(state, &thread.environment_id)? {
        return config
            .thread_url(thread.thread_id.as_str(), browser)
            .map_err(|error| error.to_string());
    }
    if browser {
        let routes = state
            .remote_routes
            .lock()
            .map_err(|_| "remote routes unavailable")?;
        let route = routes
            .get(&thread.environment_id)
            .and_then(|routes| routes.iter().find_map(remote_route_url))
            .ok_or_else(|| "Remote route unavailable".to_owned())?;
        let base = url::Url::parse(route).map_err(|_| "Remote route unavailable")?;
        Ok(format!(
            "{}{}/{}",
            base,
            encode_path_component(thread.environment_id.as_str()),
            encode_path_component(thread.thread_id.as_str())
        ))
    } else {
        Ok(format!(
            "t3code://threads/{}/{}",
            encode_path_component(thread.environment_id.as_str()),
            encode_path_component(thread.thread_id.as_str())
        ))
    }
}
#[tauri::command]
fn thread_action(
    thread: ThreadRef,
    action: String,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let url = runtime_thread_url(&state, &thread, action == "browser")?;
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
enum PreviewSource {
    Local(threads::Configuration),
    Remote(Vec<Route>),
}

/// A preview reads only a thread the hub currently lists for its environment.
fn preview_source(state: &RuntimeState, thread: &ThreadRef) -> Result<PreviewSource, String> {
    ensure_listed(state, thread)?;
    if let Some(config) = local_source(state, &thread.environment_id)? {
        return Ok(PreviewSource::Local(config));
    }
    state
        .remote_routes
        .lock()
        .map_err(|_| "remote routes unavailable")?
        .get(&thread.environment_id)
        .cloned()
        .map(PreviewSource::Remote)
        .ok_or_else(|| "Unknown environment".into())
}
#[tauri::command]
async fn get_preview(
    thread: ThreadRef,
    revision: u64,
    state: State<'_, SharedState>,
) -> Result<Option<threads::ThreadPreview>, String> {
    {
        let snapshot = state.snapshot.lock().map_err(|_| "snapshot unavailable")?;
        if !snapshot.settings.shows_previews
            || snapshot.revision != revision
            || !selected(
                &snapshot.settings.environment_selection,
                &thread.environment_id,
            )
        {
            return Ok(None);
        }
    }
    let source = preview_source(&state, &thread)?;
    let id = thread.thread_id.as_str().to_owned();
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
        let result = match source {
            PreviewSource::Local(config) => {
                threads::read_preview(&config, &id).map_err(|e| e.to_string())
            }
            PreviewSource::Remote(route) => {
                let mut attempt = state
                    .hub
                    .lock()
                    .map_err(|_| "environments unavailable")?
                    .begin_attempt(&thread.environment_id)
                    .ok_or("Unknown environment")?;
                read_remote_source(&state, &thread.environment_id, &route, RemoteReadQuery::Preview(id.clone()), &mut attempt)
                    .map(|result| result.previews.get(&id).cloned())
                    .map_err(|error| format!("{error:?}"))
            }
        };
        if state.fixture {
            record(
                "fixture_preview_completed",
                json!({"id":id,"ok":result.is_ok()}),
            );
        }
        let current = {
            let snapshot = state.snapshot.lock().map_err(|_| "snapshot unavailable")?;
            snapshot.revision == revision
                && snapshot.settings.shows_previews
                && selected(
                    &snapshot.settings.environment_selection,
                    &thread.environment_id,
                )
        };
        if !current || ensure_listed(&state, &thread).is_err() {
            return Ok(None);
        }
        preview_source(&state, &thread)?;
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
    let snap = view_snapshot(&state)?;
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
fn local_environment(config: &threads::Configuration) -> Result<EnvironmentId, String> {
    let value = std::fs::read_to_string(config.base_dir.join("environment-id"))
        .map_err(|_| "Environment identity unavailable".to_string())?;
    EnvironmentId::new(value).map_err(|e| e.to_string())
}

/// A notification click carries the environment and thread IDs it was created
/// with. Only a thread the host currently lists resolves to a destination.
fn open_notification_thread(
    state: &RuntimeState,
    environment: &str,
    thread: &str,
    open: impl FnOnce(String) -> Result<(), String>,
) -> Result<(), String> {
    let reference = ThreadRef {
        environment_id: EnvironmentId::new(environment).map_err(|e| e.to_string())?,
        thread_id: ThreadId::new(thread).map_err(|e| e.to_string())?,
    };
    open(runtime_thread_url(state, &reference, false)?)
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
async fn pair_environment(
    environment: EnvironmentId,
    token: String,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let route = state
        .remote_routes
        .lock()
        .map_err(|_| "remote routes unavailable")?
        .get(&environment)
        .cloned()
        .ok_or("Unknown remote environment")?;
    let route = route
        .into_iter()
        .find(directly_pairable)
        .ok_or("This environment does not support direct pairing")?;
    if token.trim().is_empty() {
        return Err("Enter a pairing key".into());
    }
    let state = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || {
        pair_remote_environment(&state, &environment, &route, token)?;
        refresh_threads(&app, &state);
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Replace the saved grant with one from `code`. The attempt supersedes
/// polling first, and the live session is dropped so it cannot bypass the code.
/// A rejected code leaves the previous grant in place.
fn pair_remote_environment(
    state: &RuntimeState,
    environment: &EnvironmentId,
    route: &Route,
    code: String,
) -> Result<(), String> {
    let mut hub = state.hub.lock().map_err(|_| "environments unavailable")?;
    let mut attempt = hub
        .begin_attempt(environment)
        .ok_or("Unknown environment")?;
    let session = state
        .remote_supervisors
        .lock()
        .map_err(|_| "remote sessions unavailable")?
        .remove(environment);
    drop(session);
    state
        .remote_pairing_codes
        .lock()
        .map_err(|_| "remote pairing state unavailable")?
        .insert(environment.clone(), code);
    drop(hub);
    let result = read_remote_source(
        state,
        environment,
        std::slice::from_ref(route),
        RemoteReadQuery::Snapshot(None),
        &mut attempt,
    );
    if let Ok(mut codes) = state.remote_pairing_codes.lock() {
        codes.remove(environment);
    }
    result.map(drop).map_err(|error| pairing_failure(&error))
}

fn pairing_failure(error: &RemoteReadError) -> String {
    match error {
        RemoteReadError::PermissionDenied { .. } => {
            "The pairing key was not accepted. Create a new key in T3 Code and try again.".into()
        }
        RemoteReadError::Offline | RemoteReadError::Rpc(_) => {
            "Could not reach the environment. Check that it is running and try again.".into()
        }
        RemoteReadError::UnsupportedRoute => {
            "This environment does not support direct pairing.".into()
        }
        RemoteReadError::NeedsPairing | RemoteReadError::Protocol => {
            "Pairing did not complete. Try again with a new key.".into()
        }
    }
}

/// Remove Bar's own direct and SSH grants for one remote environment. It needs
/// no network, tunnel or stored credential, and leaves T3 Connect and other
/// environments alone. The source is invalidated before waiting for the
/// credential gate, so a queued or in-flight attempt cannot save a grant after
/// the deletion.
fn forget_remote_access(state: &RuntimeState, environment: &EnvironmentId) -> Result<(), String> {
    let routes: Vec<Route> = state
        .remote_routes
        .lock()
        .map_err(|_| "remote routes unavailable")?
        .get(environment)
        .ok_or("Unknown remote environment")?
        .iter()
        .filter(|route| directly_pairable(route))
        .cloned()
        .collect();
    if routes.is_empty() {
        return Err("This environment has no direct access to forget".into());
    }
    state
        .hub
        .lock()
        .map_err(|_| "environments unavailable")?
        .forget_access(environment);
    if let Ok(mut codes) = state.remote_pairing_codes.lock() {
        codes.remove(environment);
    }
    let session = state
        .remote_supervisors
        .lock()
        .ok()
        .and_then(|mut sessions| sessions.remove(environment));
    drop(session);
    let failures = state.credential_gates.serialize(environment, || {
        routes
            .iter()
            .filter(|route| {
                forget_saved_access(environment, route, state.remote_credentials.as_ref()).is_err()
            })
            .count()
    });
    if failures > 0 {
        return Err("Could not remove all saved access from the system keyring. Some may remain. Try again.".into());
    }
    Ok(())
}

#[tauri::command]
async fn forget_environment_access(
    environment: EnvironmentId,
    state: State<'_, SharedState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let state = Arc::clone(&state);
    let result = {
        let state = Arc::clone(&state);
        tauri::async_runtime::spawn_blocking(move || forget_remote_access(&state, &environment))
            .await
            .map_err(|error| error.to_string())?
    };
    emit_snapshot(&app, &state);
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
            (SettingsStore::defaults(settings_file.clone()), Some("Settings could not be loaded. Using defaults for this session; the original file is preserved and setting changes cannot be saved.".into()))
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

    let mut hub = EnvironmentHub::new(settings);
    let cache_path = environment_cache_path(&settings_file);
    if let Err(error) = hub.load_cache(&cache_path) {
        eprintln!("Cannot load environment cache: {error}");
    }
    let remote_credentials = remote_credential_store(fixture);
    let cloud = CloudConfig::production()
        .and_then(|config| CloudSessionController::new(config, remote_credentials.clone()))
        .ok();
    let cloud_error = cloud.is_none().then(|| {
        "T3 Connect is unavailable. Check credential storage and restart the app.".to_string()
    });
    let state = Arc::new(RuntimeState {
        config: config.clone(),
        snapshot: Mutex::new(AppSnapshot {
            startup_error,
            cloud_error,
            settings: hub.settings().clone(),
            ..Default::default()
        }),
        hub: Mutex::new(hub),
        view: Mutex::new(ViewState::default()),
        sources: Mutex::new(HashMap::new()),
        remote_routes: Mutex::new(HashMap::new()),
        remote_pairing_codes: Mutex::new(HashMap::new()),
        remote_supervisors: Mutex::new(HashMap::new()),
        ssh_options: SshConnectOptions::default(),
        remote_credentials,
        credential_gates: Default::default(),
        fixture_pairing_seeded: Default::default(),
        cloud,
        cloud_generation: Default::default(),
        favicons: Default::default(),
        usage: Mutex::new(HashMap::new()),
        limits: Mutex::new(HashMap::new()),
        frozen_now: parse_now(),
        timezone: parse_tz(),
        fixture,
        fixture_notifications: fixture
            && fixture_or_env("T3_BAR_FIXTURE_NOTIFICATIONS").as_deref() == Some("1"),
        connection: limits::LimitsConnection::with_default_transport(store),
        descriptor_machine: Default::default(),
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
            begin_cloud_login,
            cancel_cloud_login,
            disconnect_cloud,
            set_view,
            resize_panel,
            hide_panel,
            set_setting,
            set_environment_selection,
            quit_app,
            thread_action,
            new_thread,
            get_preview,
            open_limit_link,
            pair_environment,
            forget_environment_access,
            check_for_updates,
            install_update,
            update_prompt_shown,
            connect,
            reconnect,
            disconnect
        ]);
    let result = builder
        .on_page_load(|webview, payload| {
            #[cfg(target_os = "linux")]
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                linux_appearance::refresh(webview.app_handle().clone());
            }
            #[cfg(not(target_os = "linux"))]
            let _ = (webview, payload);
        })
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            platform::initialize_surface(app)?;
            make_tray(app)?;
            platform::initialize(app)?;
            #[cfg(target_os = "linux")]
            linux_appearance::watch(app.handle().clone());
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
                let Some(environment) = event
                    .notification()
                    .and_then(|n| n.extra().get("environmentId"))
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
                if let Err(error) = open_notification_thread(&state, environment, id, |url| {
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
    fn usage_response_requires_current_query_and_source_generation() {
        use bar_core::connection::catalog::EnvironmentMetadata;
        let root = std::env::temp_dir().join(format!("t3-bar-query-test-{}", uuid::Uuid::new_v4()));
        let settings = SettingsStore::load(root.join("settings.json")).unwrap();
        let mut hub = EnvironmentHub::new(settings);
        let id = EnvironmentId::new("a").unwrap();
        hub.apply_discovery(Ok(vec![EnvironmentMetadata {
            id: id.clone(),
            label: "a".into(),
            enabled: true,
            routes: vec![],
        }]));
        let stamp = hub.begin_next_source(&id, "local").unwrap();
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/multi-environment/usage/parity.json"
        ))
        .unwrap();
        let environment: usage::EnvironmentUsage =
            serde_json::from_value(corpus[0]["environments"][0].clone()).unwrap();
        let query = usage::UsageQuery {
            since_day: "2026-09-01".into(),
            until_day: "2026-09-30".into(),
            time_zone: "UTC".into(),
            resolution: "day".into(),
            since_time: None,
            until_time: None,
            model_filter: None,
        };
        let contribution = usage::UsageContribution::bind(query.clone(), environment).unwrap();
        let mut current = query.clone();
        current.model_filter = Some(usage::UsageModelFilter {
            include: Some(vec![usage::UsageModelSelector {
                provider: None,
                model: "other".into(),
            }]),
            exclude: None,
        });
        assert!(!apply_usage_contribution(
            &mut hub,
            &stamp,
            &current,
            contribution.clone(),
            1
        ));
        assert!(apply_usage_contribution(
            &mut hub,
            &stamp,
            &query,
            contribution.clone(),
            1
        ));
        hub.begin_next_source(&id, "local").unwrap();
        assert!(!apply_usage_contribution(
            &mut hub,
            &stamp,
            &query,
            contribution,
            2
        ));
        assert!(matches!(
            hub.sources()[&id].snapshot.as_ref().unwrap().usage,
            Section::Stale { .. }
        ));
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn environment_access_matches_available_authorization_paths() {
        let direct = Route::Bearer {
            connection_id: "fixture".into(),
            http_base_url: Some("http://127.0.0.1:43101/".into()),
        };
        assert_eq!(
            environment_access(&[direct.clone()]),
            EnvironmentAccess::Direct
        );
        assert_eq!(
            environment_access(&[Route::Relay]),
            EnvironmentAccess::Cloud
        );
        assert_eq!(
            environment_access(&[direct, Route::Relay]),
            EnvironmentAccess::DirectOrCloud
        );
        assert_eq!(
            environment_access(&[Route::Ssh {
                connection_id: "fixture".into(),
                target: None
            }]),
            EnvironmentAccess::Unsupported
        );
        assert_eq!(environment_access(&[]), EnvironmentAccess::Unsupported);
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
            hub: Mutex::new(EnvironmentHub::new(settings)),
            snapshot: Mutex::new(AppSnapshot::default()),
            view: Mutex::new(ViewState::default()),
            sources: Mutex::new(HashMap::new()),
            remote_routes: Mutex::new(HashMap::new()),
            remote_pairing_codes: Mutex::new(HashMap::new()),
            remote_supervisors: Mutex::new(HashMap::new()),
            ssh_options: SshConnectOptions::default(),
            remote_credentials: remote_credential_store(true),
            credential_gates: Default::default(),
            fixture_pairing_seeded: Default::default(),
            cloud: Some(
                CloudSessionController::new(
                    CloudConfig::production().unwrap(),
                    remote_credential_store(true),
                )
                .unwrap(),
            ),
            cloud_generation: Default::default(),
            favicons: Default::default(),
            usage: Mutex::new(HashMap::new()),
            limits: Mutex::new(HashMap::new()),
            frozen_now: None,
            timezone: chrono_tz::UTC,
            fixture: true,
            fixture_notifications: false,
            connection: limits::LimitsConnection::with_default_transport(Arc::new(
                FixtureCredentials::default(),
            )),
            descriptor_machine: Default::default(),
            last_port: std::sync::atomic::AtomicU16::new(49216),
            connection_generation: std::sync::atomic::AtomicU64::new(1),
            connection_gate: tokio::sync::Mutex::new(()),
            thread_busy: Default::default(),
            usage_busy: Default::default(),
            limits_busy: Default::default(),
        });
        assert_eq!(view_snapshot(&state).unwrap().cloud_status, "signedOut");
        cloud_error(&state, Some("Sign in failed."));
        assert_eq!(
            view_snapshot(&state).unwrap().cloud_error.as_deref(),
            Some("Sign in failed.")
        );
        invalidate_cloud_sources(&state);
        assert_eq!(view_snapshot(&state).unwrap().cloud_status, "signedOut");
        let id = EnvironmentId::new("remote").unwrap();
        {
            let mut hub = state.hub.lock().unwrap();
            hub.apply_discovery(Ok(vec![EnvironmentMetadata {
                id: id.clone(),
                label: "Remote".into(),
                enabled: true,
                routes: vec![],
            }]));
        }
        let unavailable = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = unavailable.local_addr().unwrap();
        drop(unavailable);
        let routes = vec![
            Route::Primary {
                http_base_url: format!("http://{address}/"),
            },
            Route::Relay,
        ];
        let mut attempt = state.hub.lock().unwrap().begin_attempt(&id).unwrap();
        assert!(matches!(
            read_remote_source(&state, &id, &routes, RemoteReadQuery::Snapshot(None), &mut attempt),
            Err(RemoteReadError::NeedsPairing)
        ));
        let mut attempt = state.hub.lock().unwrap().begin_attempt(&id).unwrap();
        let routes = vec![
            Route::Relay,
            Route::Primary {
                http_base_url: format!("http://{address}/"),
            },
        ];
        assert!(matches!(
            read_remote_source(&state, &id, &routes, RemoteReadQuery::Snapshot(None), &mut attempt),
            Err(RemoteReadError::NeedsPairing)
        ));
        assert!(retry_remote_route(&RemoteReadError::NeedsPairing));
        assert!(!retry_remote_route(&RemoteReadError::PermissionDenied {
            threads: true,
            usage: true,
            limits: true,
        }));
        for (error, expected) in [
            (RemoteReadError::NeedsPairing, "needsAccess"),
            (RemoteReadError::UnsupportedRoute, "unsupported"),
            (RemoteReadError::Offline, "offline"),
        ] {
            let attempt = state.hub.lock().unwrap().begin_attempt(&id).unwrap();
            apply_remote_error(&state, &attempt, error);
            let view = view_snapshot(&state).unwrap();
            assert_eq!(
                view.environments
                    .iter()
                    .find(|item| item.id == id)
                    .unwrap()
                    .status,
                expected
            );
        }
        {
            let mut hub = state.hub.lock().unwrap();
            hub.apply_discovery(Ok(vec![EnvironmentMetadata {
                id: id.clone(),
                label: "Remote".into(),
                enabled: true,
                routes: vec![],
            }]));
            hub.begin_next_source(&id, "old-principal").unwrap();
            let current = hub.begin_next_source(&id, "new-principal").unwrap();
            hub.apply_threads(&current, vec![], 1, true);
        }
        let mut hub = state.hub.lock().unwrap();
        let attempt = hub.begin_attempt(&id).unwrap();
        let current = hub.sources()[&id].stamp.clone().unwrap();
        let newer = SourceStamp {
            generation: current.generation + 1,
            ..current
        };
        assert!(hub.begin_source(newer.clone()));
        hub.apply_threads(&newer, vec![], 2, true);
        drop(hub);
        apply_remote_error(
            &state,
            &attempt,
            RemoteReadError::PermissionDenied {
                threads: true,
                usage: true,
                limits: true,
            },
        );
        assert!(matches!(
            state.hub.lock().unwrap().sources()[&id]
                .snapshot
                .as_ref()
                .unwrap()
                .threads,
            Section::Ready { .. }
        ));
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

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../crates/bar-connection/examples/remote_fixture_server.rs"]
mod ssh_fixture_server;

#[cfg(test)]
mod remote_session_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    use super::ssh_fixture_server as fixture_server;

    /// Fake OpenSSH: answers the metadata probe and relays each `-L` forward
    /// to the fixture server named in the sibling `upstream` file. It records
    /// its pid in `pid-<local port>`.
    const FAKE_SSH: &str = r#"#!/usr/bin/python3
import os, socket, sys, threading
here = os.path.dirname(os.path.abspath(__file__))
if '-L' not in sys.argv:
    print('state:123:3773'); sys.exit(0)
local = sys.argv[sys.argv.index('-L') + 1].split(':')
upstream = int(open(os.path.join(here, 'upstream')).read())
open(os.path.join(here, 'pid-' + local[1]), 'w').write(str(os.getpid()))
listener = socket.socket(); listener.bind((local[0], int(local[1]))); listener.listen()
print('debug1: Local forwarding listening on 127.0.0.1 port ' + local[1] + '.', file=sys.stderr, flush=True)
def pump(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data: break
            b.sendall(data)
    except OSError: pass
    for s in (a, b):
        try: s.shutdown(socket.SHUT_RDWR)
        except OSError: pass
while True:
    client, _ = listener.accept()
    remote = socket.create_connection(('127.0.0.1', upstream))
    threading.Thread(target=pump, args=(client, remote), daemon=True).start()
    threading.Thread(target=pump, args=(remote, client), daemon=True).start()
"#;

    fn fixture(environment: &'static str, label: &'static str) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    loop {
                        let (stream, _) = listener.accept().await.unwrap();
                        tokio::spawn(fixture_server::handle(stream, environment, label, 60));
                    }
                });
        });
        port
    }

    fn pids(dir: &Path) -> Vec<(u16, String)> {
        let mut found: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.unwrap().file_name().into_string().ok()?;
                let port = name.strip_prefix("pid-")?.parse().ok()?;
                Some((port, std::fs::read_to_string(dir.join(&name)).ok()?))
            })
            .collect();
        found.sort();
        found
    }

    fn spawned_since(dir: &Path, known: &[(u16, String)]) -> Vec<(u16, String)> {
        pids(dir)
            .into_iter()
            .filter(|entry| !known.contains(entry))
            .collect()
    }

    fn alive(pid: &str) -> bool {
        std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    fn all_gone(dir: &Path) -> bool {
        (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(20));
            pids(dir).iter().all(|(_, pid)| !alive(pid))
        })
    }

    fn ssh_state(root: &Path, options: SshConnectOptions) -> SharedState {
        let settings = SettingsStore::load(root.join("settings.json")).unwrap();
        Arc::new(RuntimeState {
            config: threads::Configuration {
                base_dir: root.to_path_buf(),
            },
            hub: Mutex::new(EnvironmentHub::new(settings)),
            snapshot: Mutex::new(AppSnapshot::default()),
            view: Mutex::new(ViewState::default()),
            sources: Mutex::new(HashMap::new()),
            remote_routes: Mutex::new(HashMap::new()),
            remote_pairing_codes: Mutex::new(HashMap::new()),
            remote_supervisors: Mutex::new(HashMap::new()),
            ssh_options: options,
            remote_credentials: remote_credential_store(true),
            credential_gates: Default::default(),
            fixture_pairing_seeded: Default::default(),
            cloud: None,
            cloud_generation: Default::default(),
            favicons: Default::default(),
            usage: Mutex::new(HashMap::new()),
            limits: Mutex::new(HashMap::new()),
            frozen_now: None,
            timezone: chrono_tz::UTC,
            fixture: true,
            fixture_notifications: false,
            connection: limits::LimitsConnection::with_default_transport(Arc::new(
                FixtureCredentials::default(),
            )),
            descriptor_machine: Default::default(),
            last_port: std::sync::atomic::AtomicU16::new(49216),
            connection_generation: std::sync::atomic::AtomicU64::new(1),
            connection_gate: tokio::sync::Mutex::new(()),
            thread_busy: Default::default(),
            usage_busy: Default::default(),
            limits_busy: Default::default(),
        })
    }

    fn route(port: Option<u16>) -> Route {
        Route::Ssh {
            connection_id: "ssh-fixture".into(),
            target: Some(bar_core::connection::catalog::SshTarget {
                alias: "box".into(),
                hostname: "host.example".into(),
                username: None,
                port,
            }),
        }
    }

    fn poll(
        state: &RuntimeState,
        id: &EnvironmentId,
        route: &Route,
    ) -> Result<bar_core::remote::RemoteReadResult, RemoteReadError> {
        let mut attempt = state.hub.lock().unwrap().begin_attempt(id).unwrap();
        read_remote_route(state, id, route, RemoteReadQuery::Snapshot(None), &mut attempt)
    }

    #[test]
    fn ssh_route_pairs_polls_reuses_its_tunnel_restores_and_reaps() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("ssh-fake");
        std::fs::write(&program, FAKE_SSH).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let wrong = fixture("someone-else", "Other");
        let right = fixture("remote-b", "Remote B");
        let upstream =
            |port: u16| std::fs::write(dir.path().join("upstream"), port.to_string()).unwrap();
        let options = SshConnectOptions {
            ssh_program: program,
            command_timeout: Duration::from_secs(5),
            tunnel_start_timeout: Duration::from_secs(5),
            ..Default::default()
        };
        let root = tempfile::tempdir().unwrap();
        let state = ssh_state(root.path(), options);
        let id = EnvironmentId::new("remote-b").unwrap();
        let ssh = route(None);
        state
            .hub
            .lock()
            .unwrap()
            .apply_discovery(Ok(vec![EnvironmentMetadata {
                id: id.clone(),
                label: "Remote B".into(),
                enabled: true,
                routes: vec![ssh.clone()],
            }]));
        assert_eq!(
            environment_access(&[ssh.clone()]),
            EnvironmentAccess::Direct
        );

        upstream(wrong);
        assert!(matches!(
            poll(&state, &id, &ssh),
            Err(RemoteReadError::Protocol)
        ));
        assert_eq!(pids(dir.path()).len(), 1);
        assert!(
            all_gone(dir.path()),
            "wrong descriptor must reap the tunnel"
        );

        upstream(right);
        assert!(matches!(
            poll(&state, &id, &ssh),
            Err(RemoteReadError::NeedsPairing)
        ));
        assert_eq!(pids(dir.path()).len(), 2);
        assert!(
            all_gone(dir.path()),
            "missing pairing code must reap the tunnel"
        );

        state
            .remote_pairing_codes
            .lock()
            .unwrap()
            .insert(id.clone(), "JKLM2345NPQR".into());
        let before = pids(dir.path());
        let read = poll(&state, &id, &ssh).expect("pairs through the tunnel and polls");
        assert_eq!(read.threads.len(), 1);
        state.remote_pairing_codes.lock().unwrap().clear();
        let paired = spawned_since(dir.path(), &before);
        assert_eq!(paired.len(), 1);
        assert!(alive(&paired[0].1));

        assert!(poll(&state, &id, &ssh).is_ok());
        assert_eq!(
            pids(dir.path()).len(),
            3,
            "a usable session must not open another tunnel"
        );

        state.remote_supervisors.lock().unwrap().clear();
        assert!(all_gone(dir.path()));
        let before = pids(dir.path());
        assert!(
            poll(&state, &id, &ssh).is_ok(),
            "saved grant restores without a code"
        );
        let restored = spawned_since(dir.path(), &before);
        assert_eq!(restored.len(), 1);
        assert_ne!(
            paired[0].0, restored[0].0,
            "reconnect used a different ephemeral port"
        );
        assert!(alive(&restored[0].1));

        assert!(matches!(
            poll(&state, &id, &route(Some(2222))),
            Err(RemoteReadError::NeedsPairing)
        ));
        assert!(
            state.remote_supervisors.lock().unwrap().is_empty(),
            "a different target must not reuse the previous session"
        );
        assert!(all_gone(dir.path()));
    }

    #[test]
    fn ssh_access_requires_a_target() {
        let missing = Route::Ssh {
            connection_id: "ssh-fixture".into(),
            target: None,
        };
        assert_eq!(
            environment_access(&[missing]),
            EnvironmentAccess::Unsupported
        );
        assert_eq!(
            environment_access(&[route(None)]),
            EnvironmentAccess::Direct
        );
    }

    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    /// Loopback server that issues `tok-<code>` for each pairing code. It
    /// rejects `REJECTED2345`, can hold token responses, and counts exchanges.
    struct TokenServer {
        port: u16,
        exchanges: Arc<AtomicUsize>,
        hold: Arc<AtomicBool>,
        ttl_seconds: Arc<AtomicU64>,
    }

    fn token_server(environment: &'static str) -> TokenServer {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server = TokenServer {
            port: listener.local_addr().unwrap().port(),
            exchanges: Arc::default(),
            hold: Arc::default(),
            ttl_seconds: Arc::new(AtomicU64::new(3600)),
        };
        let (exchanges, hold, ttl) = (
            server.exchanges.clone(),
            server.hold.clone(),
            server.ttl_seconds.clone(),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (exchanges, hold, ttl) = (exchanges.clone(), hold.clone(), ttl.clone());
                std::thread::spawn(move || {
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while !request.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).is_err() {
                            return;
                        }
                        request.push(byte[0]);
                    }
                    let head = String::from_utf8_lossy(&request).into_owned();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")?
                                .parse()
                                .ok()
                        })
                        .unwrap_or(0usize);
                    let mut body = vec![0; length];
                    if stream.read_exact(&mut body).is_err() {
                        return;
                    }
                    let body = String::from_utf8_lossy(&body).into_owned();
                    let (status, payload) = if head.starts_with("GET /.well-known/t3/environment") {
                        (
                            "200 OK",
                            format!(
                                r#"{{"environmentId":"{environment}","label":"Test","platform":{{"os":"linux","arch":"x64"}},"serverVersion":"1","capabilities":{{}}}}"#
                            ),
                        )
                    } else if head.starts_with("POST /oauth/token") {
                        exchanges.fetch_add(1, Ordering::SeqCst);
                        while hold.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        let code = body
                            .split('&')
                            .find_map(|pair| pair.strip_prefix("subject_token="))
                            .unwrap_or_default();
                        if code == "REJECTED2345" {
                            ("400 Bad Request", "{}".into())
                        } else {
                            (
                                "200 OK",
                                format!(
                                    r#"{{"access_token":"tok-{code}","token_type":"Bearer","expires_in":{},"scope":"orchestration:read diagnostics:read"}}"#,
                                    ttl.load(Ordering::SeqCst)
                                ),
                            )
                        }
                    } else {
                        ("404 Not Found", "{}".into())
                    };
                    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                });
            }
        });
        server
    }

    fn direct_route(server: &TokenServer) -> Route {
        Route::Primary {
            http_base_url: format!("http://127.0.0.1:{}/", server.port),
        }
    }

    fn register(state: &RuntimeState, environment: &EnvironmentId, routes: Vec<Route>) {
        state
            .hub
            .lock()
            .unwrap()
            .apply_discovery(Ok(vec![EnvironmentMetadata {
                id: environment.clone(),
                label: environment.to_string(),
                enabled: true,
                routes: routes.clone(),
            }]));
        state
            .remote_routes
            .lock()
            .unwrap()
            .insert(environment.clone(), routes);
    }

    fn register_all(state: &RuntimeState, entries: &[(&EnvironmentId, Vec<Route>)]) {
        state.hub.lock().unwrap().apply_discovery(Ok(entries
            .iter()
            .map(|(id, routes)| EnvironmentMetadata {
                id: (*id).clone(),
                label: id.to_string(),
                enabled: true,
                routes: routes.clone(),
            })
            .collect()));
        let mut remote = state.remote_routes.lock().unwrap();
        for (id, routes) in entries {
            remote.insert((*id).clone(), routes.clone());
        }
    }

    fn thread_row(environment: &EnvironmentId, id: &str) -> ScopedThread {
        let item = serde_json::from_value(json!({
            "id":id, "projectId":"p", "projectTitle":"p", "title":id,
            "createdAt":"2024-01-01T00:00:00Z", "updatedAt":"2024-01-01T00:00:00Z",
            "pendingUserInputCount":0, "pendingApprovalCount":0, "providers":[],
            "hasGoal":false, "isPinned":false, "isSettled":false, "isSnoozed":false,
            "isWoke":false, "pendingGuestMessageCount":0, "isBlockedOnThreadWait":false,
            "hasRuns":false, "status":"Approval", "priority":0, "isRunning":false,
            "isAttention":true, "showsLabelInMenuBar":true
        }))
        .unwrap();
        ScopedThread::new(environment, item).unwrap()
    }

    /// Replaces what the hub lists for a registered environment, as a refresh does.
    fn list_threads(state: &RuntimeState, environment: &EnvironmentId, ids: &[&str]) {
        let mut hub = state.hub.lock().unwrap();
        let stamp = match hub.sources()[environment].stamp.clone() {
            Some(stamp) => stamp,
            None => hub.begin_next_source(environment, "test").unwrap(),
        };
        let rows = ids.iter().map(|id| thread_row(environment, id)).collect();
        hub.apply_threads(&stamp, rows, 1, true).unwrap();
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Runtime::new().unwrap()
    }

    fn grant_with(
        state: &RuntimeState,
        runtime: &tokio::runtime::Runtime,
        id: &EnvironmentId,
        route: &Route,
        code: Option<&str>,
    ) -> Result<String, RemoteReadError> {
        let endpoint = runtime
            .block_on(VerifiedEndpoint::discover(
                remote_route_url(route).unwrap(),
                id,
            ))
            .unwrap();
        let attempt = state.hub.lock().unwrap().begin_attempt(id).unwrap();
        match code {
            Some(code) => state
                .remote_pairing_codes
                .lock()
                .unwrap()
                .insert(id.clone(), code.into()),
            None => state.remote_pairing_codes.lock().unwrap().remove(id),
        };
        load_or_pair_grant(state, runtime, id, &endpoint, &attempt)
            .map(|grant| grant.principal_identity().as_key())
    }

    #[test]
    fn explicit_pairing_code_replaces_saved_grants_and_a_rejected_code_keeps_them() {
        let server = token_server("remote-b");
        let state = ssh_state(tempfile::tempdir().unwrap().path(), Default::default());
        let id = EnvironmentId::new("remote-b").unwrap();
        let route = direct_route(&server);
        register(&state, &id, vec![route.clone()]);
        let rt = runtime();
        let grant = |code| grant_with(&state, &rt, &id, &route, code);

        assert!(matches!(grant(None), Err(RemoteReadError::NeedsPairing)));
        let first = grant(Some("ABCD2345EFGH")).unwrap();
        assert!(state.remote_pairing_codes.lock().unwrap().is_empty());
        assert_eq!(grant(None).unwrap(), first);

        let second = grant(Some("JKLM2345NPQR")).unwrap();
        assert_ne!(
            second, first,
            "a valid saved grant must not outrank the explicit code"
        );
        assert_eq!(grant(None).unwrap(), second);

        for rejected in ["REJECTED2345", "bad"] {
            assert!(matches!(
                grant(Some(rejected)),
                Err(RemoteReadError::PermissionDenied { .. })
            ));
            assert_eq!(
                grant(None).unwrap(),
                second,
                "a failed replacement keeps the previous grant"
            );
        }

        server.ttl_seconds.store(1, Ordering::SeqCst);
        let short = grant(Some("SHRT2345WXYZ")).unwrap();
        assert_ne!(short, second);
        std::thread::sleep(Duration::from_millis(1300));
        server.ttl_seconds.store(3600, Ordering::SeqCst);
        assert!(
            matches!(grant(None), Err(RemoteReadError::NeedsPairing)),
            "an expired saved grant asks for access instead of denying"
        );
        let renewed = grant(Some("NEWW2345CADE")).unwrap();
        assert_ne!(
            renewed, short,
            "an expired grant is replaced by the explicit code"
        );
        assert_eq!(grant(None).unwrap(), renewed);
    }

    #[test]
    fn expired_direct_access_lets_a_later_route_serve_but_a_server_denial_stops() {
        assert!(retry_remote_route(&map_load_error(
            ConnectionError::GrantExpired
        )));
        assert!(!retry_remote_route(&map_load_error(
            ConnectionError::PermissionDenied
        )));
        assert!(!retry_remote_route(&map_error(
            ConnectionError::PairingRejected
        )));

        let server = token_server("remote-b");
        let state = ssh_state(tempfile::tempdir().unwrap().path(), Default::default());
        let id = EnvironmentId::new("remote-b").unwrap();
        let direct = direct_route(&server);
        register(&state, &id, vec![direct.clone(), Route::Relay]);
        let rt = runtime();
        server.ttl_seconds.store(1, Ordering::SeqCst);
        grant_with(&state, &rt, &id, &direct, Some("SHRT2345WXYZ")).unwrap();
        std::thread::sleep(Duration::from_millis(1300));
        let mut attempt = state.hub.lock().unwrap().begin_attempt(&id).unwrap();
        let result = read_remote_source(&state, &id, &[direct, Route::Relay], RemoteReadQuery::Snapshot(None), &mut attempt);
        assert!(
            matches!(result, Err(RemoteReadError::Protocol)),
            "the Relay route was tried (it has no cloud controller here): {result:?}"
        );
    }

    #[test]
    fn forgetting_removes_only_that_environments_access_and_drops_its_session() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("ssh-fake");
        std::fs::write(&program, FAKE_SSH).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            dir.path().join("upstream"),
            fixture("remote-b", "Remote B").to_string(),
        )
        .unwrap();
        let state = ssh_state(
            tempfile::tempdir().unwrap().path(),
            SshConnectOptions {
                ssh_program: program,
                command_timeout: Duration::from_secs(5),
                tunnel_start_timeout: Duration::from_secs(5),
                ..Default::default()
            },
        );
        let target = EnvironmentId::new("remote-b").unwrap();
        let other = EnvironmentId::new("other-env").unwrap();
        let direct_server = token_server("remote-b");
        let other_server = token_server("other-env");
        let direct = direct_route(&direct_server);
        let ssh = route(None);
        let other_direct = direct_route(&other_server);
        register_all(
            &state,
            &[
                (&target, vec![direct.clone(), ssh.clone(), Route::Relay]),
                (&other, vec![other_direct.clone()]),
            ],
        );
        let rt = runtime();
        grant_with(&state, &rt, &target, &direct, Some("ABCD2345EFGH")).unwrap();
        let other_grant =
            grant_with(&state, &rt, &other, &other_direct, Some("JKLM2345NPQR")).unwrap();
        state
            .remote_pairing_codes
            .lock()
            .unwrap()
            .insert(target.clone(), "SSHH2345CADE".into());
        poll(&state, &target, &ssh).expect("SSH access is paired and the tunnel stays up");
        let tunnel = pids(dir.path()).last().cloned().unwrap().1;
        assert!(alive(&tunnel));
        assert_eq!(state.remote_supervisors.lock().unwrap().len(), 1);
        list_threads(&state, &target, &["same"]);
        let listed = ThreadRef {
            environment_id: target.clone(),
            thread_id: ThreadId::new("same").unwrap(),
        };
        assert!(state.hub.lock().unwrap().lists_thread(&listed));
        state
            .remote_pairing_codes
            .lock()
            .unwrap()
            .insert(target.clone(), "PEND2345CADE".into());

        let exchanges = direct_server.exchanges.load(Ordering::SeqCst);
        let direct_forgotten = forget_remote_access(&state, &target);
        assert_eq!(direct_forgotten, Ok(()));
        assert!(state
            .remote_pairing_codes
            .lock()
            .unwrap()
            .get(&target)
            .is_none());
        assert!(state.remote_supervisors.lock().unwrap().is_empty());
        assert!(all_gone(dir.path()), "forgetting reaps the owned tunnel");
        assert!(!state.hub.lock().unwrap().lists_thread(&listed));
        assert_eq!(direct_server.exchanges.load(Ordering::SeqCst), exchanges);

        assert!(matches!(
            grant_with(&state, &rt, &target, &direct, None),
            Err(RemoteReadError::NeedsPairing)
        ));
        assert!(matches!(
            poll(&state, &target, &ssh),
            Err(RemoteReadError::NeedsPairing)
        ));
        assert_eq!(
            grant_with(&state, &rt, &other, &other_direct, None).unwrap(),
            other_grant,
            "another environment keeps its access"
        );
        let view = view_snapshot(&state).unwrap();
        let forgotten = view.environments.iter().find(|e| e.id == target).unwrap();
        assert_eq!(forgotten.status, "needsAccess");
        assert!(forgotten.can_forget_access);

        assert!(
            forget_remote_access(&state, &target).is_ok(),
            "forgetting is repeatable"
        );
        assert!(forget_remote_access(&state, &EnvironmentId::new("unknown").unwrap()).is_err());
        let relay_only = EnvironmentId::new("relay-only").unwrap();
        register(&state, &relay_only, vec![Route::Relay]);
        assert!(forget_remote_access(&state, &relay_only).is_err());
    }

    #[test]
    fn forgetting_works_with_the_environment_unreachable() {
        let state = ssh_state(tempfile::tempdir().unwrap().path(), Default::default());
        let id = EnvironmentId::new("remote-b").unwrap();
        let unreachable = Route::Primary {
            http_base_url: "http://127.0.0.1:1/".into(),
        };
        let ssh = route(Some(2222));
        register(&state, &id, vec![unreachable, ssh]);
        assert_eq!(forget_remote_access(&state, &id), Ok(()));
        assert!(state.remote_supervisors.lock().unwrap().is_empty());
    }

    #[test]
    fn forgetting_waits_for_a_pairing_in_flight_and_stops_queued_attempts_from_saving() {
        let server = token_server("remote-b");
        let state = ssh_state(tempfile::tempdir().unwrap().path(), Default::default());
        let id = EnvironmentId::new("remote-b").unwrap();
        let route = direct_route(&server);
        register(&state, &id, vec![route.clone()]);
        let rt = Arc::new(runtime());

        server.hold.store(true, Ordering::SeqCst);
        let pairing = {
            let (state, rt, id, route) = (state.clone(), rt.clone(), id.clone(), route.clone());
            std::thread::spawn(move || grant_with(&state, &rt, &id, &route, Some("ABCD2345EFGH")))
        };
        while server.exchanges.load(Ordering::SeqCst) == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
        let queued = {
            let (state, rt, id, route) = (state.clone(), rt.clone(), id.clone(), route.clone());
            std::thread::spawn(move || {
                let endpoint = rt
                    .block_on(VerifiedEndpoint::discover(
                        remote_route_url(&route).unwrap(),
                        &id,
                    ))
                    .unwrap();
                let attempt = state.hub.lock().unwrap().begin_attempt(&id).unwrap();
                state
                    .remote_pairing_codes
                    .lock()
                    .unwrap()
                    .insert(id.clone(), "JKLM2345NPQR".into());
                load_or_pair_grant(&state, &rt, &id, &endpoint, &attempt)
                    .map(|grant| grant.principal_identity().as_key())
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        let forgetting = {
            let (state, id) = (state.clone(), id.clone());
            std::thread::spawn(move || forget_remote_access(&state, &id))
        };
        while state
            .hub
            .lock()
            .unwrap()
            .sources()
            .get(&id)
            .and_then(|source| source.attempt_status.clone())
            != Some(AttemptStatus::NeedsPairing)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        server.hold.store(false, Ordering::SeqCst);
        let pairing = pairing.join().unwrap();
        assert!(
            pairing.is_ok(),
            "the in-flight pairing finishes before forgetting runs"
        );
        assert_eq!(forgetting.join().unwrap(), Ok(()));
        assert!(
            matches!(queued.join().unwrap(), Err(RemoteReadError::Offline)),
            "a queued attempt superseded by forgetting does not touch credentials"
        );
        assert_eq!(
            server.exchanges.load(Ordering::SeqCst),
            1,
            "the queued attempt never exchanged its code"
        );
        assert!(matches!(
            grant_with(&state, &rt, &id, &route, None),
            Err(RemoteReadError::NeedsPairing)
        ));
        assert_eq!(
            server.exchanges.load(Ordering::SeqCst),
            1,
            "polling does not re-pair"
        );
    }

    #[test]
    fn notification_and_thread_actions_resolve_one_source_per_environment() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("environment-id"), "local-env").unwrap();
        let state = ssh_state(directory.path(), Default::default());
        let local = EnvironmentId::new("local-env").unwrap();
        let remote = EnvironmentId::new("remote-b").unwrap();
        let config = threads::Configuration {
            base_dir: directory.path().to_path_buf(),
        };
        state
            .sources
            .lock()
            .unwrap()
            .insert(local.clone(), config.clone());
        let remote_routes = vec![Route::Primary {
            http_base_url: "http://127.0.0.1:4100/".into(),
        }];
        register_all(&state, &[(&local, vec![]), (&remote, remote_routes)]);
        let reference = |environment: &EnvironmentId, thread: &str| ThreadRef {
            environment_id: environment.clone(),
            thread_id: ThreadId::new(thread).unwrap(),
        };
        list_threads(&state, &local, &["same"]);
        list_threads(&state, &remote, &["same", "a b"]);
        let open = |environment: &str, thread: &str| {
            let mut opened = Vec::new();
            let result = open_notification_thread(&state, environment, thread, |url| {
                opened.push(url);
                Ok(())
            });
            (result, opened)
        };

        let (result, opened) = open("local-env", "same");
        assert_eq!(result, Ok(()));
        assert_eq!(opened, vec![config.thread_url("same", false).unwrap()]);
        assert_eq!(opened, vec!["t3code://threads/local-env/same".to_owned()]);
        let (_, opened) = open("remote-b", "same");
        assert_eq!(opened, vec!["t3code://threads/remote-b/same".to_owned()]);
        let (_, opened) = open("remote-b", "a b");
        assert_eq!(opened, vec!["t3code://threads/remote-b/a%20b".to_owned()]);
        assert_eq!(
            runtime_thread_url(&state, &reference(&remote, "same"), true),
            Ok("http://127.0.0.1:4100/remote-b/same".to_owned())
        );
        assert_eq!(
            runtime_thread_url(&state, &reference(&local, "same"), false),
            Ok("t3code://threads/local-env/same".to_owned())
        );

        for (environment, thread) in [
            ("remote-b", "missing"),
            ("unknown-env", "same"),
            ("local-env", "a b"),
        ] {
            let (result, opened) = open(environment, thread);
            assert_eq!(
                result,
                Err("Unknown thread".to_owned()),
                "{environment}/{thread}"
            );
            assert!(opened.is_empty());
        }
        assert!(open(" ", "same").0.is_err() && open("remote-b", "").0.is_err());
        assert_eq!(
            open_notification_thread(&state, "remote-b", "same", |_| Err(
                "opener unavailable".into()
            )),
            Err("opener unavailable".to_owned())
        );

        list_threads(&state, &remote, &["same"]);
        for browser in [false, true] {
            assert_eq!(
                runtime_thread_url(&state, &reference(&remote, "a b"), browser),
                Err("Unknown thread".to_owned()),
                "a thread that left the listing cannot be opened or copied"
            );
        }
        let (result, opened) = open("remote-b", "a b");
        assert_eq!(result, Err("Unknown thread".to_owned()));
        assert!(opened.is_empty(), "a stale notification opens nothing");
        assert_eq!(open("remote-b", "same").0, Ok(()));

        list_threads(&state, &local, &[]);
        assert_eq!(
            open("local-env", "same").0,
            Err("Unknown thread".to_owned()),
            "deleting a local thread does not affect the same ID elsewhere"
        );
        assert_eq!(open("remote-b", "same").0, Ok(()));
        list_threads(&state, &local, &["same"]);

        assert_eq!(forget_remote_access(&state, &remote), Ok(()));
        let (result, opened) = open("remote-b", "same");
        assert_eq!(
            result,
            Err("Unknown thread".to_owned()),
            "forgotten sources stop routing"
        );
        assert!(opened.is_empty());
        assert_eq!(open("local-env", "same").0, Ok(()));
    }

    #[test]
    fn previews_read_only_threads_the_current_listing_names() {
        let directory = tempfile::tempdir().unwrap();
        let state = ssh_state(directory.path(), Default::default());
        let local = EnvironmentId::new("local-env").unwrap();
        let remote = EnvironmentId::new("remote-b").unwrap();
        state.sources.lock().unwrap().insert(
            local.clone(),
            threads::Configuration {
                base_dir: directory.path().to_path_buf(),
            },
        );
        let routes = vec![Route::Primary {
            http_base_url: "http://127.0.0.1:4100/".into(),
        }];
        register_all(&state, &[(&local, vec![]), (&remote, routes.clone())]);
        let reference = |environment: &EnvironmentId, thread: &str| ThreadRef {
            environment_id: environment.clone(),
            thread_id: ThreadId::new(thread).unwrap(),
        };
        let unknown = |thread: ThreadRef| {
            matches!(
                preview_source(&state, &thread),
                Err(error) if error == "Unknown thread"
            )
        };

        assert!(
            unknown(reference(&remote, "same")),
            "a route alone does not authorize a preview"
        );
        list_threads(&state, &remote, &["same"]);
        assert!(matches!(
            preview_source(&state, &reference(&remote, "same")),
            Ok(PreviewSource::Remote(found)) if found == routes
        ));
        assert!(
            unknown(reference(&remote, "never-listed")),
            "an arbitrary ID is not previewed from a listed environment"
        );
        assert!(
            unknown(reference(&local, "same")),
            "the same ID listed by another environment does not authorize this one"
        );
        list_threads(&state, &local, &["same"]);
        assert!(matches!(
            preview_source(&state, &reference(&local, "same")),
            Ok(PreviewSource::Local(_))
        ));

        list_threads(&state, &remote, &[]);
        assert!(
            unknown(reference(&remote, "same")),
            "a deleted remote thread is not previewed from stale authorization"
        );
        assert!(matches!(
            preview_source(&state, &reference(&local, "same")),
            Ok(PreviewSource::Local(_))
        ));
    }
}
