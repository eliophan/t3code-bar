use chrono::{DateTime, Utc};
use regex::Regex;
use rusqlite::{Connection, OpenFlags, Row};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, path::PathBuf, time::Duration};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    Missing,
    Unavailable,
    IncompatibleSchema,
    Busy,
    InvalidData,
    MissingEnvironment,
    MissingRuntime,
}
impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DomainError {}
fn sql_error(e: rusqlite::Error) -> DomainError {
    use rusqlite::ErrorCode::*;
    match e {
        rusqlite::Error::SqliteFailure(e, _) => match e.code {
            DatabaseBusy | DatabaseLocked => DomainError::Busy,
            CannotOpen | PermissionDenied | AuthorizationForStatementDenied | SystemIoFailure => {
                DomainError::Unavailable
            }
            _ => DomainError::InvalidData,
        },
        _ => DomainError::InvalidData,
    }
}
#[derive(Debug, Clone)]
pub struct Configuration {
    pub base_dir: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInfo {
    pub environment_id: String,
    pub label: String,
    pub machine: String,
    pub present: bool,
    pub source: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Runtime {
    pid: Option<i64>,
    port: Option<i64>,
    origin: Option<String>,
}
impl Configuration {
    pub fn from_environment(explicit: Option<PathBuf>) -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let expand = |s: String| {
            let s = s.trim();
            if s == "~" {
                home.clone()
            } else if let Some(rest) = s.strip_prefix("~/") {
                home.join(rest)
            } else {
                PathBuf::from(s)
            }
        };
        let base_dir = explicit
            .or_else(|| {
                std::env::var("T3_CODE_BAR_STATE_DIR")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(expand)
            })
            .or_else(|| {
                std::env::var("T3CODE_HOME")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| expand(s).join("userdata"))
            })
            .unwrap_or_else(|| home.join(".t3/userdata"));
        Self { base_dir }
    }
    fn runtime(&self) -> Option<Runtime> {
        serde_json::from_slice(&std::fs::read(self.base_dir.join("server-runtime.json")).ok()?).ok()
    }
    pub fn is_server_online(&self) -> bool {
        self.runtime()
            .and_then(|r| r.pid)
            .filter(|p| *p > 0 && *p <= i32::MAX as i64)
            .is_some_and(|p| unsafe { libc::kill(p as libc::pid_t, 0) == 0 })
    }

    /// Returns the number of non-local environments known by T3 Code's cache.
    ///
    /// This is discovery metadata only. The Bar still connects exclusively to
    /// the local T3 Code server; cached rows must never be treated as live
    /// remote connections.
    pub fn linked_environment_count(&self) -> usize {
        self.environment_catalog().iter().filter(|e| e.source == "linked").count()
    }

    pub fn environment_catalog(&self) -> Vec<EnvironmentInfo> {
        let path = self.base_dir.join("tauri-bar-settings.json.environments-cache-v1");
        let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return Vec::new() };
        value.get("sources").and_then(serde_json::Value::as_array).map(|sources| sources.iter().filter_map(|source| {
            let id = source.get("id")?.as_str()?.trim();
            let machine = source.get("machine").and_then(serde_json::Value::as_str).unwrap_or("unknown");
            Some(EnvironmentInfo {
                environment_id: id.to_owned(),
                label: source.get("label").and_then(serde_json::Value::as_str).unwrap_or(id).to_owned(),
                machine: machine.to_owned(),
                present: source.get("present").and_then(serde_json::Value::as_bool).unwrap_or(false),
                source: if machine == "laptop" { "local" } else { "linked" }.to_owned(),
            })
        }).collect()).unwrap_or_default()
    }
    pub fn server_port(&self) -> Result<u16, DomainError> {
        let runtime = self.runtime().ok_or(DomainError::MissingRuntime)?;
        runtime
            .port
            .or_else(|| {
                runtime
                    .origin
                    .as_deref()
                    .and_then(|origin| origin.split('/').nth(2))
                    .and_then(|authority| authority.rsplit_once(':'))
                    .and_then(|(_, port)| port.parse().ok())
            })
            .filter(|port| (1..=65535).contains(port))
            .map(|port| port as u16)
            .ok_or(DomainError::MissingRuntime)
    }
    pub fn thread_url(&self, id: &str, browser: bool) -> Result<String, DomainError> {
        let env = std::fs::read_to_string(self.base_dir.join("environment-id"))
            .map_err(|_| DomainError::MissingEnvironment)?;
        let env = env.trim();
        if env.is_empty() || id.is_empty() {
            return Err(DomainError::MissingEnvironment);
        }
        fn encode(s: &str) -> String {
            s.bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect()
        }
        let prefix = if browser {
            let port = self.server_port()?;
            format!("http://127.0.0.1:{port}")
        } else {
            "t3code://threads".into()
        };
        Ok(format!("{prefix}/{}/{}", encode(env), encode(id)))
    }
}
fn open(c: &Configuration) -> Result<Connection, DomainError> {
    let p = c.base_dir.join("statev2.sqlite");
    if !p.exists() {
        return Err(DomainError::Missing);
    }
    let db = Connection::open_with_flags(
        p,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )
    .map_err(sql_error)?;
    db.busy_timeout(Duration::from_millis(250))
        .map_err(sql_error)?;
    Ok(db)
}
fn date(s: Option<String>) -> Option<DateTime<Utc>> {
    let text = s.as_deref()?;
    DateTime::parse_from_rfc3339(text)
        .or_else(|_| DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f%z"))
        .ok()
        .map(|d| d.with_timezone(&Utc))
}
fn txt(r: &Row<'_>, i: usize) -> rusqlite::Result<Option<String>> {
    use rusqlite::types::ValueRef;
    Ok(match r.get_ref(i)? {
        ValueRef::Null => None,
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        ValueRef::Integer(value) => Some(value.to_string()),
        ValueRef::Real(value) => Some(value.to_string()),
    })
}
fn dt(r: &Row<'_>, i: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    Ok(date(txt(r, i)?))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadStatus {
    Working,
    Goal,
    Approval,
    Input,
    Request,
    Woke,
    Failed,
    Waiting,
    Limited,
    Done,
    Settled,
}
impl ThreadStatus {
    pub fn priority(self) -> u8 {
        match self {
            Self::Approval => 8,
            Self::Input | Self::Request => 7,
            Self::Goal => 6,
            Self::Working => 5,
            Self::Failed | Self::Limited => 4,
            Self::Woke => 3,
            Self::Done => 1,
            _ => 0,
        }
    }
    pub fn is_running(self) -> bool {
        matches!(self, Self::Working | Self::Goal)
    }
    pub fn is_attention(self) -> bool {
        matches!(
            self,
            Self::Working
                | Self::Goal
                | Self::Approval
                | Self::Input
                | Self::Request
                | Self::Failed
                | Self::Limited
                | Self::Woke
        )
    }
    pub fn shows_label_in_menu_bar(self) -> bool {
        matches!(
            self,
            Self::Approval
                | Self::Input
                | Self::Request
                | Self::Woke
                | Self::Failed
                | Self::Limited
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadItem {
    pub id: String,
    pub project_id: String,
    pub project_title: String,
    pub title: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub latest_user_message_at: Option<DateTime<Utc>>,
    pub active_run_status: Option<String>,
    pub active_run_started_at: Option<DateTime<Utc>>,
    pub latest_run_status: Option<String>,
    pub latest_run_started_at: Option<DateTime<Utc>>,
    pub latest_run_completed_at: Option<DateTime<Utc>>,
    pub pending_user_input_count: i64,
    pub pending_approval_count: i64,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub model_name: Option<String>,
    pub providers: Vec<String>,
    pub has_goal: bool,
    pub is_pinned: bool,
    pub is_settled: bool,
    pub is_snoozed: bool,
    pub is_woke: bool,
    pub pin_order_key: Option<String>,
    pub active_order_key: Option<String>,
    pub pr_number: Option<i64>,
    pub project_icon_json: Option<String>,
    pub project_favicon_path: Option<String>,
    pub workspace_root: Option<String>,
    pub unsettled_at: Option<DateTime<Utc>>,
    pub last_visited_at: Option<DateTime<Utc>>,
    pub latest_failure_class: Option<String>,
    pub usage_limit_reset_at: Option<DateTime<Utc>>,
    pub pending_guest_message_count: i64,
    pub is_blocked_on_thread_wait: bool,
    pub has_runs: bool,
    pub status: ThreadStatus,
    pub priority: u8,
    pub is_running: bool,
    pub is_attention: bool,
    pub shows_label_in_menu_bar: bool,
    pub status_started_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSummary {
    pub status: ThreadStatus,
    pub attention_count: usize,
    pub running_count: usize,
    pub badge_count: usize,
}
impl ThreadSummary {
    pub fn new(threads: &[ThreadItem], online: bool) -> Self {
        let mut s = Self {
            status: ThreadStatus::Settled,
            attention_count: 0,
            running_count: 0,
            badge_count: 0,
        };
        if online {
            for t in threads {
                if t.priority > s.status.priority() {
                    s.status = t.status;
                }
                s.attention_count += t.is_attention as usize;
                s.running_count += t.is_running as usize;
            }
            if s.status.is_running() {
                s.badge_count = s.running_count;
            } else if s.status.is_attention() {
                s.badge_count = threads
                    .iter()
                    .filter(|t| t.priority == s.status.priority())
                    .count();
            }
        }
        s
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadSnapshot {
    pub threads: Vec<ThreadItem>,
    pub is_server_online: bool,
    pub summary: ThreadSummary,
}
fn parse_row(r: &Row<'_>, now: DateTime<Utc>) -> Result<Option<ThreadItem>, DomainError> {
    let get = || -> rusqlite::Result<_> {
        let id: String = r.get(0)?;
        let title: String = r.get(1)?;
        let project_id: String = r.get(2)?;
        let project_title: String = r.get(3)?;
        let created_at = dt(r, 4)?;
        let updated_at = dt(r, 5)?;
        let active_run_status: Option<String> = txt(r, 6)?;
        let active_run_started_at = dt(r, 7)?;
        let latest_run_status: Option<String> = txt(r, 8)?;
        let latest_run_started_at = dt(r, 9)?;
        let latest_run_completed_at = dt(r, 10)?;
        let pending_user_input_count: i64 = r.get(11)?;
        let pending_approval_count: i64 = r.get(12)?;
        let branch = txt(r, 13)?;
        let worktree_path = txt(r, 14)?;
        let model_name: Option<String> = txt(r, 15)?;
        let pr_number = r.get(16)?;
        let is_pinned = r.get::<_, i64>(17)? > 0;
        let is_settled = r.get::<_, i64>(18)? > 0;
        let snoozed_until = dt(r, 19)?;
        let has_goal = r.get::<_, i64>(20)? > 0;
        let snoozed_at = dt(r, 21)?;
        let pin_order_key = txt(r, 22)?;
        let active_order_key = txt(r, 23)?;
        let history: Option<String> = txt(r, 24)?;
        let latest_user_message_at = dt(r, 25)?;
        let project_icon_json = txt(r, 26)?;
        let project_favicon_path = txt(r, 27)?;
        let workspace_root = txt(r, 28)?;
        let unsettled_at = dt(r, 29)?;
        let last_visited_at = dt(r, 30)?;
        let session_status: Option<String> = txt(r, 31)?;
        let session_updated_at = dt(r, 32)?;
        let failure: Option<String> = txt(r, 33)?;
        let failure: Option<serde_json::Value> =
            failure.and_then(|s| serde_json::from_str(&s).ok());
        let latest_failure_class = failure
            .as_ref()
            .and_then(|v| v.get("class"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let usage_limit_reset_at = if latest_failure_class.as_deref() == Some("usage_limit") {
            date(
                failure
                    .as_ref()
                    .and_then(|v| v.get("resetAt"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            )
        } else {
            None
        };
        let pending_guest_message_count: i64 = r.get(34)?;
        let has_runs = r.get::<_, i64>(35)? > 0;
        let is_blocked_on_thread_wait = r.get::<_, i64>(36)? > 0;
        Ok((
            id,
            title,
            project_id,
            project_title,
            created_at,
            updated_at,
            active_run_status,
            active_run_started_at,
            latest_run_status,
            latest_run_started_at,
            latest_run_completed_at,
            pending_user_input_count,
            pending_approval_count,
            branch,
            worktree_path,
            model_name,
            pr_number,
            is_pinned,
            is_settled,
            snoozed_until,
            has_goal,
            snoozed_at,
            pin_order_key,
            active_order_key,
            history,
            latest_user_message_at,
            project_icon_json,
            project_favicon_path,
            workspace_root,
            unsettled_at,
            last_visited_at,
            session_status,
            session_updated_at,
            latest_failure_class,
            usage_limit_reset_at,
            pending_guest_message_count,
            has_runs,
            is_blocked_on_thread_wait,
        ))
    };
    let (
        id,
        title,
        project_id,
        project_title,
        created_at,
        updated_at,
        active_run_status,
        active_run_started_at,
        latest_run_status,
        latest_run_started_at,
        latest_run_completed_at,
        pending_user_input_count,
        pending_approval_count,
        branch,
        worktree_path,
        model_name,
        pr_number,
        is_pinned,
        is_settled,
        snoozed_until,
        has_goal,
        snoozed_at,
        pin_order_key,
        active_order_key,
        history,
        latest_user_message_at,
        project_icon_json,
        project_favicon_path,
        workspace_root,
        unsettled_at,
        last_visited_at,
        session_status,
        session_updated_at,
        latest_failure_class,
        usage_limit_reset_at,
        pending_guest_message_count,
        has_runs,
        is_blocked_on_thread_wait,
    ) = get().map_err(sql_error)?;
    let created_at = created_at.ok_or(DomainError::InvalidData)?;
    let updated_at = updated_at.ok_or(DomainError::InvalidData)?;
    let providers = history
        .map(|h| {
            h.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_else(|| model_name.clone().into_iter().collect());
    let completed_while_snoozed = latest_run_status.as_deref() == Some("completed")
        && snoozed_at
            .zip(latest_run_completed_at)
            .is_some_and(|(a, b)| b > a);
    let fresh_failure = session_status.as_deref() == Some("error")
        && snoozed_at
            .map(|s| session_updated_at.is_some_and(|u| u > s))
            .unwrap_or(true);
    let raised = pending_approval_count > 0
        || pending_user_input_count > 0
        || fresh_failure
        || completed_while_snoozed;
    let is_snoozed = snoozed_until.is_some_and(|s| s > now && !raised);
    if is_snoozed {
        return Ok(None);
    }
    let woke_at = snoozed_until.and_then(|s| {
        if raised {
            if completed_while_snoozed {
                latest_run_completed_at
            } else {
                session_updated_at.or(snoozed_at)
            }
        } else if s <= now {
            Some(s)
        } else {
            None
        }
    });
    let is_woke = woke_at.is_some_and(|w| last_visited_at.map(|v| v < w).unwrap_or(true));
    let status = if pending_approval_count > 0 {
        ThreadStatus::Approval
    } else if pending_user_input_count > 0 {
        ThreadStatus::Input
    } else if pending_guest_message_count > 0 {
        ThreadStatus::Request
    } else if active_run_status.as_deref() == Some("running") && is_blocked_on_thread_wait {
        ThreadStatus::Waiting
    } else if active_run_status
        .as_deref()
        .is_some_and(|s| ["starting", "preparing", "running", "waiting"].contains(&s))
    {
        if has_goal {
            ThreadStatus::Goal
        } else {
            ThreadStatus::Working
        }
    } else if matches!(latest_run_status.as_deref(), Some("failed" | "error")) {
        if latest_failure_class.as_deref() == Some("usage_limit") {
            ThreadStatus::Limited
        } else {
            ThreadStatus::Failed
        }
    } else if is_woke {
        ThreadStatus::Woke
    } else if !has_runs {
        ThreadStatus::Waiting
    } else if latest_run_status.as_deref() == Some("completed")
        && latest_run_completed_at
            .zip(last_visited_at)
            .is_some_and(|(c, v)| c > v)
    {
        ThreadStatus::Done
    } else {
        ThreadStatus::Settled
    };
    let status_started_at = if status.is_running() {
        Some(
            active_run_started_at
                .or(latest_run_started_at)
                .unwrap_or(updated_at),
        )
    } else {
        None
    };
    Ok(Some(ThreadItem {
        id,
        project_id,
        project_title,
        title,
        created_at,
        updated_at,
        latest_user_message_at,
        active_run_status,
        active_run_started_at,
        latest_run_status,
        latest_run_started_at,
        latest_run_completed_at,
        pending_user_input_count,
        pending_approval_count,
        branch,
        worktree_path,
        model_name,
        providers,
        has_goal,
        is_pinned,
        is_settled,
        is_snoozed,
        is_woke,
        pin_order_key,
        active_order_key,
        pr_number,
        project_icon_json,
        project_favicon_path,
        workspace_root,
        unsettled_at,
        last_visited_at,
        latest_failure_class,
        usage_limit_reset_at,
        pending_guest_message_count,
        is_blocked_on_thread_wait,
        has_runs,
        status,
        priority: status.priority(),
        is_running: status.is_running(),
        is_attention: status.is_attention(),
        shows_label_in_menu_bar: status.shows_label_in_menu_bar(),
        status_started_at,
    }))
}
fn order(a: &ThreadItem, b: &ThreadItem) -> Ordering {
    b.is_pinned.cmp(&a.is_pinned).then_with(|| {
        if a.is_pinned {
            match (&a.pin_order_key, &b.pin_order_key) {
                (Some(x), Some(y)) if x != y => x.cmp(y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                _ => b
                    .created_at
                    .cmp(&a.created_at)
                    .then_with(|| a.id.cmp(&b.id)),
            }
        } else {
            match (&a.active_order_key, &b.active_order_key) {
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (Some(x), Some(y)) => x.cmp(y).then_with(|| a.id.cmp(&b.id)),
                (None, None) => {
                    let epoch = DateTime::from_timestamp(0, 0).unwrap();
                    let ea = a.created_at.max(a.unsettled_at.unwrap_or(epoch));
                    let eb = b.created_at.max(b.unsettled_at.unwrap_or(epoch));
                    eb.cmp(&ea).then_with(|| a.id.cmp(&b.id))
                }
            }
        }
    })
}
pub fn read_threads(c: &Configuration, now: DateTime<Utc>) -> Result<ThreadSnapshot, DomainError> {
    let db = open(c)?;
    let mut stmt = match db.prepare(include_str!("threads.sql")) {
        Ok(s) => s,
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::Unknown => db
            .prepare(include_str!("threads_fallback.sql"))
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(ref v, _)
                    if v.code == rusqlite::ErrorCode::Unknown =>
                {
                    DomainError::IncompatibleSchema
                }
                _ => sql_error(e),
            })?,
        Err(e) => return Err(sql_error(e)),
    };
    let mut rows = stmt.query([]).map_err(sql_error)?;
    let mut threads = Vec::new();
    while let Some(row) = rows.next().map_err(sql_error)? {
        if let Some(t) = parse_row(row, now)? {
            threads.push(t)
        }
    }
    threads.sort_by(order);
    let online = c.is_server_online();
    let summary = ThreadSummary::new(&threads, online);
    Ok(ThreadSnapshot {
        threads,
        is_server_online: online,
        summary,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadPreview {
    pub text: String,
    pub updated_at: Option<DateTime<Utc>>,
}
pub fn preview_excerpt(markdown: &str) -> String {
    let code = Regex::new(r"(?s)```.*?(```|$)")
        .unwrap()
        .replace_all(markdown, "");
    let links = Regex::new(r"\[([^\]]+)\]\([^)]*\)")
        .unwrap()
        .replace_all(&code, "$1");
    let heading = Regex::new(r"^#{1,6}\s+").unwrap();
    let quote = Regex::new(r"^>\s?").unwrap();
    let bullet = Regex::new(r"^[-*]\s+").unwrap();
    let lines: Vec<String> = links
        .split('\n')
        .filter_map(|raw| {
            let s = heading.replace(raw.trim(), "");
            let s = quote.replace(&s, "");
            let s = bullet.replace(&s, "• ");
            let s = s.replace("**", "").replace("__", "").replace('`', "");
            let s = s.trim().to_owned();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        })
        .collect();
    let mut truncated = lines.len() > 5;
    let mut result = lines.iter().take(5).cloned().collect::<Vec<_>>().join("\n");
    let graphemes: Vec<&str> = result.graphemes(true).collect();
    if graphemes.len() > 360 {
        let cut = graphemes[..360].concat();
        result = cut.rfind(' ').map(|i| cut[..i].to_owned()).unwrap_or(cut);
        truncated = true;
    }
    if truncated {
        result.push('…')
    }
    result
}
pub fn read_preview(c: &Configuration, id: &str) -> Result<Option<ThreadPreview>, DomainError> {
    let db = open(c)?;
    let mut stmt=db.prepare("SELECT substr(json_extract(payload_json, '$.text'), 1, 4000), updated_at FROM orchestration_v2_projection_messages WHERE thread_id = ?1 AND role = 'assistant' AND COALESCE(json_extract(payload_json, '$.text'), '') != '' ORDER BY created_at DESC LIMIT 1").map_err(|e|match e {rusqlite::Error::SqliteFailure(ref v,_) if v.code==rusqlite::ErrorCode::Unknown=>DomainError::IncompatibleSchema,_=>sql_error(e)})?;
    let mut rows = stmt.query([id]).map_err(sql_error)?;
    if let Some(row) = rows.next().map_err(sql_error)? {
        let text = preview_excerpt(&row.get::<_, String>(0).map_err(sql_error)?);
        if text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ThreadPreview {
                text,
                updated_at: dt(row, 1).map_err(sql_error)?,
            }))
        }
    } else {
        Ok(None)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderKind {
    OpenAI,
    Claude,
    Gemini,
    Grok,
    Cursor,
    OpenCode,
    Antigravity,
    Fx,
    Kiro,
    Pi,
    Muse,
    Unknown,
}
impl ProviderKind {
    pub fn resolve(raw: &str) -> Self {
        let n = raw.to_lowercase();
        for (p, k) in [
            ("codex", Self::OpenAI),
            ("claudeagent", Self::Claude),
            ("cursor", Self::Cursor),
            ("grok", Self::Grok),
            ("opencode", Self::OpenCode),
            ("antigravity", Self::Antigravity),
            ("fx", Self::Fx),
            ("kiro", Self::Kiro),
            ("pi", Self::Pi),
            ("muse", Self::Muse),
        ] {
            if n == p || n.strip_prefix(p).is_some_and(|s| s.starts_with('_')) {
                return k;
            }
        }
        if ["grok", "xai"].iter().any(|s| n.contains(s)) {
            Self::Grok
        } else if ["claude", "anthropic", "sonnet", "opus", "haiku"]
            .iter()
            .any(|s| n.contains(s))
        {
            Self::Claude
        } else if ["gemini", "google"].iter().any(|s| n.contains(s)) {
            Self::Gemini
        } else if ["gpt", "openai", "codex"].iter().any(|s| n.contains(s))
            || n.starts_with("o3")
            || n.starts_with("o4")
        {
            Self::OpenAI
        } else {
            Self::Unknown
        }
    }
    pub fn resolve_unique(raw: &[String]) -> Vec<Self> {
        let mut out = Vec::new();
        for r in raw {
            let k = Self::resolve(r);
            if !out.contains(&k) {
                out.push(k)
            }
        }
        if out.is_empty() {
            out.push(Self::OpenAI)
        }
        out
    }
    pub fn display_name(self) -> &'static str {
        match self {
            Self::OpenAI => "Codex",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Grok => "Grok",
            Self::Cursor => "Cursor",
            Self::OpenCode => "OpenCode",
            Self::Antigravity => "Antigravity",
            Self::Fx => "fx",
            Self::Kiro => "Kiro",
            Self::Pi => "Pi",
            Self::Muse => "Muse",
            Self::Unknown => "Other",
        }
    }
}
pub fn export_json(
    snapshot: &ThreadSnapshot,
    preview: Option<&ThreadPreview>,
) -> serde_json::Value {
    let threads:Vec<_>=snapshot.threads.iter().map(|t|serde_json::json!({"id":t.id,"title":t.title,"projectId":t.project_id,"projectTitle":t.project_title,"status":t.status,"priority":t.priority,"isRunning":t.is_running,"isAttention":t.is_attention,"isPinned":t.is_pinned,"isWoke":t.is_woke,"isSnoozed":t.is_snoozed,"providers":t.providers,"pendingApprovalCount":t.pending_approval_count,"pendingUserInputCount":t.pending_user_input_count,"pendingGuestMessageCount":t.pending_guest_message_count,"isBlockedOnThreadWait":t.is_blocked_on_thread_wait,"hasRuns":t.has_runs,"pinOrderKey":t.pin_order_key,"activeOrderKey":t.active_order_key})).collect();
    serde_json::json!({"schemaVersion":1,"threads":threads,"summary":snapshot.summary,"preview":preview.map(|p|p.text.as_str()).unwrap_or("")})
}
