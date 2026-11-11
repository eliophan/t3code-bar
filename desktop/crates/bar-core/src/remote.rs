//! Source-scoped read runtime for paired direct and relay environments.
//!
//! RPC values are parsed at this boundary. The host receives only typed,
//! environment-bound contributions and never sees a token or raw route.

use bar_connection::{rpc::{ReadEvent, ReadSession, RpcError}, relay::RelayConnection, ssh::SshTunnel, Grant, VerifiedEndpoint};
use bar_identity::{EnvironmentId, EnvironmentMachineKind};
use bar_limits::{LimitsContribution, LiveLimitsParser};
use bar_threads::{ThreadItem, ThreadStatus};
use bar_usage::{EnvironmentUsage, UsageContribution, UsageQuery, UsageSummary};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{collections::{HashMap, VecDeque}, time::{Duration, Instant}};

const READ_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceStamp {
    pub environment: EnvironmentId,
    pub generation: u64,
    pub principal: String,
}

#[derive(Debug)]
pub enum RemoteReadError {
    Rpc(RpcError),
    Protocol,
    NeedsPairing,
    CredentialUnavailable,
    UnsupportedRoute,
    Offline,
    PermissionDenied { threads: bool, usage: bool, limits: bool },
}
impl From<RpcError> for RemoteReadError {
    fn from(error: RpcError) -> Self { Self::Rpc(error) }
}

#[derive(Debug)]
pub struct RemoteReadResult {
    pub stamp: SourceStamp,
    pub threads: Vec<ThreadItem>,
    pub usage: Option<UsageContribution>,
    pub limits: Option<LimitsContribution>,
    pub previews: HashMap<String, bar_threads::ThreadPreview>,
    pub usage_permission_denied: bool,
    pub limits_permission_denied: bool,
    pub machine: Option<EnvironmentMachineKind>,
    pub environment_icon_override: Option<EnvironmentMachineKind>,
}

pub enum RemoteConnection {
    Direct { endpoint: VerifiedEndpoint, grant: Grant },
    Relay(RelayConnection),
    /// The tunnel is owned for as long as the connection lives. Dropping the
    /// connection kills and reaps the OpenSSH child.
    Ssh { endpoint: VerifiedEndpoint, grant: Grant, tunnel: SshTunnel },
}
impl RemoteConnection {
    fn parts(&self) -> (&VerifiedEndpoint, &Grant) {
        match self {
            Self::Direct { endpoint, grant } | Self::Ssh { endpoint, grant, .. } => (endpoint, grant),
            Self::Relay(connection) => (&connection.endpoint, &connection.grant),
        }
    }
}

pub struct SourceSupervisor {
    connection: RemoteConnection,
    stamp: SourceStamp,
    label: String,
    session: Option<ReadSession>,
}
impl SourceSupervisor {
    pub fn new(connection: RemoteConnection, stamp: SourceStamp, label: impl Into<String>) -> Self {
        Self { connection, stamp, label: label.into(), session: None }
    }

    pub fn stamp(&self) -> &SourceStamp { &self.stamp }

    pub fn usable(&mut self) -> bool {
        let tunnel_running = match &mut self.connection {
            RemoteConnection::Ssh { tunnel, .. } => tunnel.is_running(),
            _ => true,
        };
        tunnel_running && self.connection.parts().1.expires_at() > Instant::now() + Duration::from_secs(15)
    }

    pub async fn read(&mut self, query: Option<&UsageQuery>) -> Result<RemoteReadResult, RemoteReadError> {
        self.read_requested(query, None).await
    }

    pub async fn read_preview(&mut self, thread_id: &str) -> Result<RemoteReadResult, RemoteReadError> {
        self.read_requested(None, Some(thread_id)).await
    }

    async fn read_requested(&mut self, query: Option<&UsageQuery>, preview_thread: Option<&str>) -> Result<RemoteReadResult, RemoteReadError> {
        let mut session = match self.session.take() {
            Some(session) => session,
            None => {
                let (endpoint, grant) = self.connection.parts();
                ReadSession::connect(endpoint, grant).await?
            }
        };
        let result = self.read_session(&mut session, query, preview_thread).await;
        match result {
            Ok(result) => {
                self.session = Some(session);
                Ok(result)
            }
            Err(error) => {
                let _ = session.cancel_all().await;
                let _ = session.close().await;
                Err(error)
            }
        }
    }

    async fn read_session(
        &self,
        session: &mut ReadSession,
        query: Option<&UsageQuery>,
        preview_thread: Option<&str>,
    ) -> Result<RemoteReadResult, RemoteReadError> {
        let (orchestration_read, diagnostics_read) = {
            let (_, grant) = self.connection.parts();
            (grant.orchestration_read, grant.diagnostics_read)
        };
        if !orchestration_read {
            return Err(RemoteReadError::PermissionDenied { threads: true, usage: !diagnostics_read, limits: !diagnostics_read });
        }
        let shell = session.subscribe_shell(None).await?;
        let config = session.get_config().await?;
        let config_stream = session.subscribe_server_config().await?;
        let usage_request = match (diagnostics_read, query) {
            (true, Some(query)) => {
                let model_filter = query
                    .model_filter
                    .as_ref()
                    .map(|filter| serde_json::to_value(filter).map_err(|_| RemoteReadError::Protocol))
                    .transpose()?;
                Some(session.get_usage_summary(
                    &query.since_day,
                    &query.until_day,
                    &query.time_zone,
                    &query.resolution,
                    query.since_time.as_deref(),
                    query.until_time.as_deref(),
                    model_filter.as_ref(),
                ).await?)
            }
            _ => None,
        };
        let mut streams = HashMap::from([(shell, StreamKind::Shell), (config_stream, StreamKind::Config)]);
        let mut unary = HashMap::from([(config, UnaryKind::Config)]);
        if let Some(request) = usage_request { unary.insert(request, UnaryKind::Usage); }
        let mut threads = Vec::new();
        let mut projections = HashMap::new();
        let mut projection_queue = VecDeque::new();
        let mut limits_parser = LiveLimitsParser::default();
        let mut limits_seen = false;
        let mut previews = HashMap::new();
        let mut usage = None;
        let mut usage_denied = false;
        let mut limits_denied = false;
        let mut usage_failed = false;
        let mut shell_done = false;
        let mut config_stream_done = false;
        let mut usage_done = usage_request.is_none();
        let mut config_icon = None;
        let deadline = Instant::now() + READ_TIMEOUT;

        while !(shell_done && config_stream_done && usage_done && projections.is_empty() && projection_queue.is_empty()) {
            let event = session.next_event_until(deadline).await?;
            match event {
                ReadEvent::Chunk { request_id, values } => match streams.get(&request_id) {
                    Some(StreamKind::Shell) => {
                        for value in values {
                            let value = value.decode::<Value>().map_err(|_| RemoteReadError::Protocol)?;
                            let mut ids = Vec::new();
                            collect_threads(&value, &mut threads, &mut ids, &HashMap::new());
                            collect_shell_previews(&value, &mut previews);
                            for id in ids.into_iter().filter(|id| preview_thread == Some(id.as_str())) {
                                if !projection_queue.contains(&id) && !projections.values().any(|existing| existing == &id) {
                                    projection_queue.push_back(id);
                                }
                            }
                        }
                        shell_done = true;
                        streams.remove(&request_id);
                        session.cancel(request_id).await?;
                    }
                    Some(StreamKind::Config) => {
                        let mut snapshot_received = false;
                        for value in values {
                            let value = value.decode::<Value>().map_err(|_| RemoteReadError::Protocol)?;
                            apply_config_event(&mut limits_parser, &value, &mut config_icon)?;
                            if value.get("type").and_then(Value::as_str) == Some("snapshot") {
                                limits_seen = true;
                                snapshot_received = true;
                            }
                        }
                        if snapshot_received {
                            config_stream_done = true;
                            streams.remove(&request_id);
                            session.cancel(request_id).await?;
                            if unary.contains_key(&config) {
                                session.cancel(config).await?;
                                unary.remove(&config);
                            }
                        }
                    }
                    None => {}
                },
                ReadEvent::Success { request_id, value } => {
                    let value = value.decode::<Value>().map_err(|_| RemoteReadError::Protocol)?;
                    match unary.remove(&request_id) {
                        Some(UnaryKind::Config) => {
                            apply_config_reply(&mut limits_parser, &value, &mut config_icon)?;
                            limits_seen = true;
                        }
                        Some(UnaryKind::Projection) => {
                            let thread_id = projections.remove(&request_id).ok_or(RemoteReadError::Protocol)?;
                            if let Some((text, updated_at)) = latest_projection_text(&value) {
                                let excerpt = bar_threads::preview_excerpt(&text);
                                if !excerpt.is_empty() {
                                    previews.insert(thread_id, bar_threads::ThreadPreview {
                                        text: excerpt,
                                        updated_at: updated_at.and_then(|value| serde_json::from_value(value).ok()),
                                    });
                                }
                            }
                        }
                        Some(UnaryKind::Usage) => {
                            usage = parse_usage(&self.stamp.environment, &self.label, &value, query.ok_or(RemoteReadError::Protocol)?)?;
                            usage_done = true;
                        }
                        None => {}
                    }
                }
                ReadEvent::PermissionDenied { request_id } => {
                    if streams.remove(&request_id).is_some() {
                        if request_id == shell { return Err(RemoteReadError::PermissionDenied { threads: true, usage: !diagnostics_read, limits: !diagnostics_read }); }
                        config_stream_done = true;
                        limits_denied = true;
                    }
                    if unary.remove(&request_id).is_some() {
                        if request_id == config { limits_denied = true; }
                        if Some(request_id) == usage_request { usage_done = true; usage_denied = true; }
                        projections.remove(&request_id);
                    }
                }
                ReadEvent::Failure { request_id, .. } => {
                    if streams.remove(&request_id).is_some() {
                        if request_id == shell { return Err(RemoteReadError::Protocol); }
                        config_stream_done = true;
                    }
                    if unary.remove(&request_id).is_some() {
                        if Some(request_id) == usage_request { usage_done = true; usage_failed = true; }
                        projections.remove(&request_id);
                    }
                }
            }
            if projections.is_empty() {
                if let Some(id) = projection_queue.pop_front() {
                    let request = session.get_thread_projection(&id).await?;
                    projections.insert(request, id);
                    unary.insert(request, UnaryKind::Projection);
                }
            }
        }
        let limits = limits_seen.then(|| limits_parser.contribution(self.stamp.environment.clone()));
        if (limits.is_none() && !limits_denied) || usage_failed {
            return Err(RemoteReadError::Protocol);
        }
        session.cancel_all().await?;

        threads.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).reverse().then_with(|| a.id.cmp(&b.id)));
        threads.dedup_by(|a, b| a.id == b.id);
        previews.retain(|id, _| threads.iter().any(|thread| thread.id == *id));
        let machine = self.connection.parts().0.descriptor().machine;
        Ok(RemoteReadResult {
            stamp: self.stamp.clone(),
            threads,
            usage,
            limits,
            previews,
            usage_permission_denied: usage_denied,
            limits_permission_denied: limits_denied,
            machine,
            environment_icon_override: config_icon,
        })
    }
}

#[derive(Clone, Copy)]
enum StreamKind { Shell, Config }
#[derive(Clone, Copy)]
enum UnaryKind { Config, Usage, Projection }

#[derive(Clone)]
struct ProjectInfo {
    title: String,
    workspace_root: Option<String>,
    favicon_path: Option<String>,
    icon_json: Option<String>,
}

fn collect_threads(
    value: &Value,
    out: &mut Vec<ThreadItem>,
    ids: &mut Vec<String>,
    projects: &HashMap<String, ProjectInfo>,
) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect_threads(item, out, ids, projects)),
        Value::Object(object) => {
            if object.get("kind").and_then(Value::as_str) == Some("snapshot") {
                let snapshot = object.get("snapshot").unwrap_or(value);
                let mut project_map = projects.clone();
                if let Some(items) = snapshot.get("projects").and_then(Value::as_array) {
                    for project in items {
                        if let (Some(id), Some(title)) = (
                            project.get("id").and_then(Value::as_str),
                            project.get("title").and_then(Value::as_str),
                        ) {
                            project_map.insert(id.to_owned(), ProjectInfo {
                                title: title.to_owned(),
                                workspace_root: string(project.get("workspaceRoot")),
                                favicon_path: string(project.get("faviconPath")),
                                icon_json: project.get("projectIcon").filter(|icon| !icon.is_null()).map(Value::to_string),
                            });
                        }
                    }
                }
                if let Some(child) = snapshot.get("threads") {
                    collect_threads(child, out, ids, &project_map);
                }
                return;
            }
            if object.get("kind").and_then(Value::as_str) == Some("thread.updated") {
                if let Some(thread) = object.get("thread") {
                    if let Some(thread) = shell_thread(thread, projects) {
                        if needs_projection(&thread, object.get("thread").unwrap_or(&Value::Null)) {
                            ids.push(thread.id.clone());
                        }
                        out.push(thread);
                    }
                }
                return;
            }
            if let Ok(thread) = serde_json::from_value::<ThreadItem>(value.clone()) {
                if !thread.is_settled && !thread.is_snoozed {
                    out.push(thread);
                }
                return;
            }
            if object.contains_key("projectId") && object.contains_key("title") {
                if let Some(thread) = shell_thread(value, projects) {
                    if needs_projection(&thread, value) {
                        ids.push(thread.id.clone());
                    }
                    out.push(thread);
                }
                return;
            }
            if let Some(id) = object
                .get("threadId")
                .or_else(|| object.get("id"))
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
            {
                ids.push(id.to_owned());
            }
            for key in ["threads", "items", "value", "payload", "shell"] {
                if let Some(child) = object.get(key) {
                    collect_threads(child, out, ids, projects);
                }
            }
        }
        _ => {}
    }
}

fn needs_projection(_thread: &ThreadItem, value: &Value) -> bool {
    value.pointer("/latestVisibleMessage/role").and_then(Value::as_str) != Some("assistant")
}

fn shell_thread(value: &Value, projects: &HashMap<String, ProjectInfo>) -> Option<ThreadItem> {
    let object = value.as_object()?;
    let id = object.get("id")?.as_str()?.to_owned();
    if id.starts_with("thread:")
        || ["archivedAt", "deletedAt", "settledAt", "delegatedFrom"]
            .iter().any(|key| object.get(*key).is_some_and(|value| !value.is_null()))
        || object.get("settledOverride").and_then(Value::as_str) == Some("settled")
        || value.pointer("/lineage/sideChat").and_then(Value::as_bool) == Some(true)
        || value.pointer("/lineage/relationshipToParent").and_then(Value::as_str) == Some("subagent")
    {
        return None;
    }
    let project_id = object.get("projectId")?.as_str()?.to_owned();
    let title = object.get("title")?.as_str()?.to_owned();
    let created_at = date(object.get("createdAt"))?;
    let updated_at = date(object.get("updatedAt"))?;
    let active_run_status = object
        .get("activityRunStatus")
        .or_else(|| object.get("status"))
        .and_then(Value::as_str)
        .filter(|status| *status != "idle")
        .map(str::to_owned);
    let latest_run_status = object.get("status").and_then(Value::as_str).and_then(|status| {
        (status != "idle").then(|| match status {
            "completed" => "completed".to_owned(),
            "failed" => "failed".to_owned(),
            other => other.to_owned(),
        })
    });
    let request_kind = object
        .get("pendingRuntimeRequest")
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str);
    let pending_user_input_count = i64::from(request_kind == Some("user_input"));
    let pending_approval_count = i64::from(request_kind.is_some() && request_kind != Some("user_input"));
    let pending_guest_message_count = object
        .get("pendingGuestMessageCount")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let has_goal = object.get("goal").is_some_and(|goal| !goal.is_null())
        || object
            .get("hasActionableProposedPlan")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    let has_runs = object.get("latestRunId").is_some_and(|id| !id.is_null())
        || object.get("itemCount").and_then(Value::as_i64).unwrap_or(0) > 0;
    let blocked = object
        .get("blockedOnThreadWait")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let last_visited_at = date(object.get("lastVisitedAt"));
    let snoozed_until = date_like(object.get("snoozedUntil"), &created_at);
    let now = current_time_like(&created_at);
    let is_snoozed = snoozed_until.is_some_and(|at| at > now)
        && pending_user_input_count == 0
        && pending_approval_count == 0;
    if is_snoozed {
        return None;
    }
    let is_woke = snoozed_until.is_some_and(|at| at <= now && last_visited_at.map_or(true, |visited| visited < at));
    let latest_failure_class = object
        .get("lastErrorClass")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let usage_limit_reset_at = date(object.get("usageLimitResetAt"));
    let is_settled = false;
    let status = if pending_approval_count > 0 {
        ThreadStatus::Approval
    } else if pending_user_input_count > 0 {
        ThreadStatus::Input
    } else if pending_guest_message_count > 0 {
        ThreadStatus::Request
    } else if blocked {
        ThreadStatus::Waiting
    } else if active_run_status
        .as_deref()
        .is_some_and(|status| ["preparing", "starting", "running", "waiting"].contains(&status))
    {
        if has_goal { ThreadStatus::Goal } else { ThreadStatus::Working }
    } else if latest_failure_class.as_deref() == Some("usage_limit") {
        ThreadStatus::Limited
    } else if latest_run_status.as_deref() == Some("failed") {
        ThreadStatus::Failed
    } else if is_woke {
        ThreadStatus::Woke
    } else if !has_runs {
        ThreadStatus::Waiting
    } else if latest_run_status.as_deref() == Some("completed") {
        ThreadStatus::Done
    } else {
        ThreadStatus::Settled
    };
    let providers = object
        .get("providerInstanceHistory")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_else(|| object.get("providerInstanceId").and_then(Value::as_str).into_iter().map(str::to_owned).collect());
    let model_name = object
        .get("modelSelection").and_then(|value| value.get("model"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let active_run_started_at = date(object.get("activityRunStartedAt").or_else(|| object.get("latestRunStartedAt")));
    Some(ThreadItem {
        id,
        project_id: project_id.clone(),
        project_title: projects.get(&project_id).map(|project| project.title.clone()).unwrap_or_else(|| project_id.clone()),
        title,
        created_at,
        updated_at,
        latest_user_message_at: date(object.get("latestUserMessageAt")),
        active_run_status,
        active_run_started_at,
        latest_run_status,
        latest_run_started_at: date(object.get("latestRunStartedAt")),
        latest_run_completed_at: date(object.get("latestRunCompletedAt")),
        pending_user_input_count,
        pending_approval_count,
        branch: string(object.get("branch")),
        worktree_path: string(object.get("worktreePath")),
        model_name,
        providers,
        has_goal,
        is_pinned: object.get("pinnedAt").is_some_and(|at| !at.is_null()),
        is_settled,
        is_snoozed,
        is_woke,
        pin_order_key: string(object.get("pinOrderKey")),
        active_order_key: string(object.get("activeOrderKey")),
        pr_number: object.get("branchPullRequest").and_then(|value| value.get("number")).and_then(Value::as_i64),
        project_icon_json: projects.get(&project_id).and_then(|project| project.icon_json.clone()),
        project_favicon_path: projects.get(&project_id).and_then(|project| project.favicon_path.clone()),
        workspace_root: projects.get(&project_id).and_then(|project| project.workspace_root.clone()),
        unsettled_at: date(object.get("unsettledAt")),
        last_visited_at,
        latest_failure_class,
        usage_limit_reset_at,
        pending_guest_message_count,
        is_blocked_on_thread_wait: blocked,
        has_runs,
        status,
        priority: status.priority(),
        is_running: status.is_running(),
        is_attention: status.is_attention(),
        shows_label_in_menu_bar: status.shows_label_in_menu_bar(),
        status_started_at: status.is_running().then_some(active_run_started_at).flatten(),
    })
}

fn collect_shell_previews(value: &Value, previews: &mut HashMap<String, bar_threads::ThreadPreview>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect_shell_previews(item, previews)),
        Value::Object(object) => {
            if let (Some(id), Some(message)) = (
                object.get("id").and_then(Value::as_str),
                object.get("latestVisibleMessage"),
            ) {
                if message.get("role").and_then(Value::as_str) == Some("assistant") {
                    if let Some(text) = message.get("text").and_then(Value::as_str) {
                        let text = bar_threads::preview_excerpt(text);
                        if !text.is_empty() {
                            previews.insert(id.to_owned(), bar_threads::ThreadPreview {
                                text,
                                updated_at: date(message.get("updatedAt")),
                            });
                        }
                    }
                }
            }
            for key in ["snapshot", "threads", "archivedThreads", "thread"] {
                if let Some(child) = object.get(key) {
                    collect_shell_previews(child, previews);
                }
            }
        }
        _ => {}
    }
}

fn latest_projection_text(value: &Value) -> Option<(String, Option<serde_json::Value>)> {
    let mut candidates = Vec::new();
    if let Some(items) = value.get("messages").and_then(Value::as_array) {
        candidates.extend(items.iter());
    }
    candidates
        .into_iter()
        .filter_map(|item| {
            if item.get("role").and_then(Value::as_str) != Some("assistant") {
                return None;
            }
            let text = item.get("text").and_then(Value::as_str).filter(|text| !text.trim().is_empty())?;
            Some((text.to_owned(), item.get("createdAt").cloned(), item.get("updatedAt").cloned()))
        })
        .max_by_key(|(_, created_at, _)| created_at.as_ref().and_then(Value::as_str).unwrap_or_default().to_owned())
        .map(|(text, _, updated_at)| (text, updated_at))
}

fn date<T: DeserializeOwned>(value: Option<&Value>) -> Option<T> {
    value.and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn date_like<T: DeserializeOwned>(value: Option<&Value>, _: &T) -> Option<T> {
    date(value)
}

fn current_time_like<T: From<std::time::SystemTime>>(_: &T) -> T {
    std::time::SystemTime::now().into()
}

fn string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn apply_config_event(
    parser: &mut LiveLimitsParser,
    value: &Value,
    icon: &mut Option<EnvironmentMachineKind>,
) -> Result<(), RemoteReadError> {
    *icon = config_icon(value).or(*icon);
    parser.apply(value).map(|_| ()).map_err(|_| RemoteReadError::Protocol)
}

fn apply_config_reply(
    parser: &mut LiveLimitsParser,
    value: &Value,
    icon: &mut Option<EnvironmentMachineKind>,
) -> Result<(), RemoteReadError> {
    let config = value.get("config").unwrap_or(value);
    if config.get("providers").is_none() {
        return Err(RemoteReadError::Protocol);
    }
    *icon = config_icon(config).or(*icon);
    let event = serde_json::json!({"version":1,"type":"snapshot","config":config});
    parser.apply(&event).map(|_| ()).map_err(|_| RemoteReadError::Protocol)
}

fn config_icon(value: &Value) -> Option<EnvironmentMachineKind> {
    value
        .pointer("/config/settings/environmentIcon")
        .or_else(|| value.pointer("/settings/environmentIcon"))
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn parse_usage(environment: &EnvironmentId, label: &str, value: &Value, query: &UsageQuery) -> Result<Option<UsageContribution>, RemoteReadError> {
    let value = value.get("summary").unwrap_or(value);
    for (key, expected) in [
        ("resolution", Some(query.resolution.as_str())),
        ("sinceTime", query.since_time.as_deref()),
        ("untilTime", query.until_time.as_deref()),
    ] {
        if let Some(actual) = value.get(key) {
            if actual.as_str() != expected {
                return Err(RemoteReadError::Protocol);
            }
        }
    }
    let summary: UsageSummary = serde_json::from_value(value.clone()).map_err(|_| RemoteReadError::Protocol)?;
    if query.resolution == "hour" && summary.buckets.iter().any(|bucket| bucket.hour_start.is_none()) {
        return Err(RemoteReadError::Protocol);
    }
    let environment = EnvironmentUsage { environment_id: environment.clone(), label: label.to_owned(), summary };
    UsageContribution::bind(query.clone(), environment).map(Some).map_err(|_| RemoteReadError::Protocol)
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../bar-connection/examples/remote_fixture_server.rs"]
mod fixture_server;

#[cfg(test)]
mod tests {
    use super::*;
    use bar_identity::EnvironmentId;
    use serde_json::json;

    #[tokio::test]
    async fn lists_without_hydrating_history_and_fetches_only_the_requested_preview() {
        use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
        let projections = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let observed = projections.clone();
        let server = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(super::fixture_server::handle_counting_projections(stream, "remote", "Remote", 20, observed.clone()));
            }
        });
        let environment = EnvironmentId::new("remote").unwrap();
        let endpoint = VerifiedEndpoint::discover(&origin, &environment).await.unwrap();
        let grant = endpoint.pair("ABCD2345EFGH").await.unwrap();
        let stamp = SourceStamp { environment, generation: 1, principal: "synthetic".into() };
        let mut source = SourceSupervisor::new(RemoteConnection::Direct { endpoint, grant }, stamp, "Remote");
        let listing = source.read(None).await.unwrap();
        assert_eq!(listing.threads.len(), 1);
        assert_eq!(projections.load(Ordering::SeqCst), 0, "listing must not download conversation history");
        let preview = source.read_preview("same-thread-id").await.unwrap();
        assert_eq!(preview.previews["same-thread-id"].text, "Preview for Remote");
        assert_eq!(projections.load(Ordering::SeqCst), 1);
        let unlisted = source.read_preview("not-listed").await.unwrap();
        assert!(!unlisted.previews.contains_key("not-listed"));
        assert_eq!(projections.load(Ordering::SeqCst), 1, "unlisted IDs must not trigger a history read");
        server.abort();
    }

    #[test]
    fn remote_sidebar_excludes_hidden_threads_before_preview_reads() {
        let visible = json!({"id":"visible","projectId":"p","title":"Visible","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","status":"idle","itemCount":0});
        let mut items = vec![visible.clone()];
        for (id, patch) in [
            ("settled", json!({"settledOverride":"settled"})),
            ("settled-legacy", json!({"settledAt":"2026-01-01T00:00:00Z"})),
            ("snoozed", json!({"snoozedUntil":"2099-01-01T00:00:00Z"})),
            ("archived", json!({"archivedAt":"2026-01-01T00:00:00Z"})),
            ("deleted", json!({"deletedAt":"2026-01-01T00:00:00Z"})),
            ("delegated", json!({"delegatedFrom":{"threadId":"parent"}})),
            ("side-chat", json!({"lineage":{"sideChat":true}})),
            ("subagent", json!({"lineage":{"relationshipToParent":"subagent"}})),
            ("thread:internal", json!({})),
        ] {
            let mut item = visible.clone();
            item["id"] = json!(id);
            item.as_object_mut().unwrap().extend(patch.as_object().unwrap().clone());
            items.push(item);
        }
        // The typed legacy payload must obey the same visibility rules.
        let typed = shell_thread(&visible, &HashMap::new()).unwrap();
        for field in ["isSettled", "isSnoozed"] {
            let mut item = serde_json::to_value(&typed).unwrap();
            item["id"] = json!(field);
            item[field] = json!(true);
            items.push(item);
        }
        let envelope = json!({"kind":"snapshot","snapshot":{"threads":items,"archivedThreads":[{ "id":"archive-bucket", "projectId":"p", "title":"Archived", "createdAt":"2026-01-01T00:00:00Z", "updatedAt":"2026-01-01T00:00:00Z" }]}});
        let mut threads = Vec::new();
        let mut ids = Vec::new();
        collect_threads(&envelope, &mut threads, &mut ids, &HashMap::new());
        assert_eq!(threads.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["visible"]);
        assert_eq!(ids, ["visible"], "hidden threads cannot enqueue projection reads");
    }

    #[test]
    fn parses_colliding_thread_ids_as_source_payloads() {
        let value = json!({"threads":[{"id":"same","projectId":"p","projectTitle":"p","title":"remote","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","pendingUserInputCount":0,"pendingApprovalCount":0,"providers":[],"hasGoal":false,"isPinned":false,"isSettled":false,"isSnoozed":false,"isWoke":false,"pendingGuestMessageCount":0,"isBlockedOnThreadWait":false,"hasRuns":false,"status":"Done","priority":1,"isRunning":false,"isAttention":false,"showsLabelInMenuBar":false}]});
        let mut threads = Vec::new(); let mut ids = Vec::new(); collect_threads(&value, &mut threads, &mut ids, &HashMap::new());
        assert_eq!(threads.len(), 1); assert!(ids.is_empty());
    }

    #[test]
    fn parses_usage_and_limits_without_leaking_source_identity() {
        let env = EnvironmentId::new("remote-a").unwrap();
        let query = UsageQuery { since_day:"2026-01-01".into(), until_day:"2026-01-31".into(), time_zone:"UTC".into(), resolution:"day".into(), since_time:None, until_time:None, model_filter:None };
        let summary = json!({"contractVersion":6,"readAt":"2026-01-01T00:00:00Z","timeZone":"UTC","sinceDay":"2026-01-01","untilDay":"2026-01-31","buckets":[],"sources":[],"pricing":{"status":"fresh","source":"remote","fetchedAt":null,"knownModels":0},"scanDurationMs":1});
        assert!(parse_usage(&env, "Remote A", &summary, &query).unwrap().is_some());
        let limits = json!({"providers":[{"instanceId":"x","driver":"codex","enabled":true,"installed":true,"auth":{"status":"authenticated"},"usageLimits":{"windows":[]}}]});
        let mut parser = LiveLimitsParser::default();
        parser.apply(&json!({"version":1,"type":"snapshot","config":limits})).unwrap();
        assert_eq!(parser.contribution(env).records.len(), 1);
    }

    #[test]
    fn decodes_upstream_shell_envelope_and_projection_preview() {
        let shell = json!({
            "id":"same",
            "projectId":"p",
            "title":"remote thread",
            "providerInstanceId":"codex",
            "modelSelection":{"instanceId":"codex","model":"gpt-5.4"},
            "runtimeMode":"full-access",
            "interactionMode":"default",
            "branch":null,
            "worktreePath":null,
            "lineage":{"rootThreadId":"same","parentThreadId":null,"relationshipToParent":null},
            "forkedFrom":null,
            "activeProviderThreadId":null,
            "latestRunId":null,
            "activeRunId":null,
            "status":"idle",
            "pendingRuntimeRequest":null,
            "latestVisibleMessage":null,
            "latestUserMessageAt":null,
            "hasActionableProposedPlan":false,
            "itemCount":0,
            "visibleItemCount":0,
            "createdAt":"2026-10-01T00:00:00.000Z",
            "updatedAt":"2026-10-01T00:00:00.000Z",
            "archivedAt":null,
            "settledOverride":null,
            "settledAt":null,
            "deletedAt":null
        });
        let envelope = json!({"kind":"snapshot","snapshot":{"schemaVersion":1,"snapshotSequence":1,"projects":[{"id":"p","title":"Remote project"}],"threads":[shell],"archivedThreads":[]}});
        let mut threads = Vec::new();
        let mut ids = Vec::new();
        collect_threads(&envelope, &mut threads, &mut ids, &HashMap::new());
        assert_eq!(threads[0].id, "same");
        assert_eq!(threads[0].project_title, "Remote project");
        assert_eq!(threads[0].status, ThreadStatus::Waiting);
        let projection = json!({"thread":shell,"messages":[{"role":"assistant","text":"hello **world**","createdAt":"2026-10-01T00:01:00.000Z","updatedAt":"2026-10-01T00:01:00.000Z"}]});
        assert_eq!(bar_threads::preview_excerpt(&latest_projection_text(&projection).unwrap().0), "hello world");
    }

    #[test]
    fn config_parser_keeps_usage_limit_sources_and_icon_override() {
        let env = EnvironmentId::new("remote-a").unwrap();
        let mut parser = LiveLimitsParser::default();
        let mut icon = None;
        apply_config_event(
            &mut parser,
            &json!({"version":1,"type":"snapshot","config":{"settings":{"environmentIcon":"mac-mini"},"providers":[],"usageLimitSources":[{"id":"hub","label":"Hub","accounts":[{"id":"account","driver":"codex","email":"u@example.invalid","usageLimits":{"checkedAt":"2026-09-03T12:01:00Z","windows":[{"id":"slot","kind":"session","label":"Session","usedPercent":40}]}}]}]}}),
            &mut icon,
        )
        .unwrap();
        let contribution = parser.contribution(env);
        assert_eq!(icon, Some(EnvironmentMachineKind::MacMini));
        assert_eq!(contribution.records.len(), 1);
        assert!(contribution.records[0].is_source);
        assert_eq!(contribution.records[0].limits.as_ref().unwrap().windows[0].used_percent, 40.0);
    }

    #[test]
    fn rejects_echoed_usage_query_mismatch() {
        let env = EnvironmentId::new("remote-a").unwrap();
        let query = UsageQuery { since_day:"2026-01-01".into(), until_day:"2026-01-02".into(), time_zone:"UTC".into(), resolution:"hour".into(), since_time:Some("2026-01-01T12:00:00Z".into()), until_time:Some("2026-01-02T12:00:00Z".into()), model_filter:None };
        let summary = json!({"contractVersion":6,"readAt":"2026-01-02T12:00:00Z","timeZone":"UTC","sinceDay":"2026-01-01","untilDay":"2026-01-02","resolution":"day","buckets":[],"sources":[],"pricing":{"status":"fresh","source":"remote","fetchedAt":null,"knownModels":0},"scanDurationMs":1});
        assert!(matches!(parse_usage(&env, "Remote A", &summary, &query), Err(RemoteReadError::Protocol)));
    }
}
