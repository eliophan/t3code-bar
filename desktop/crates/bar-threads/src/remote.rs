use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteThread {
    pub id: String,
    pub project_id: String,
    pub project_title: String,
    pub title: String,
    pub status: String,
    pub is_pinned: bool,
    pub branch: Option<String>,
    pub updated_at: String,
    pub latest_user_message_at: Option<String>,
    pub latest_run_completed_at: Option<String>,
    pub active_run_started_at: Option<String>,
    pub usage_limit_reset_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteSnapshotError {
    InvalidJson,
    InvalidShape,
    MissingThreadId,
    MissingTimestamp,
}

fn string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}

fn bool(value: Option<&Value>) -> bool {
    value.and_then(Value::as_bool).unwrap_or(false)
}

pub fn decode_shell_snapshot(bytes: &[u8]) -> Result<Vec<RemoteThread>, RemoteSnapshotError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|_| RemoteSnapshotError::InvalidJson)?;
    let projects = root
        .get("projects")
        .and_then(Value::as_array)
        .ok_or(RemoteSnapshotError::InvalidShape)?;
    let project_titles = projects.iter().filter_map(|project| {
        Some((string(project.get("id"))?, string(project.get("name")).or_else(|| string(project.get("title")))?))
    }).collect::<std::collections::HashMap<_, _>>();
    let threads = root
        .get("threads")
        .and_then(Value::as_array)
        .ok_or(RemoteSnapshotError::InvalidShape)?;
    threads.iter().map(|thread| {
        let id = string(thread.get("id")).filter(|id| !id.is_empty()).ok_or(RemoteSnapshotError::MissingThreadId)?;
        let updated_at = string(thread.get("updatedAt")).filter(|value| !value.is_empty()).ok_or(RemoteSnapshotError::MissingTimestamp)?;
        let project_id = string(thread.get("projectId")).unwrap_or_default();
        Ok(RemoteThread {
            project_title: project_titles.get(&project_id).cloned().unwrap_or_else(|| "Unknown project".into()),
            project_id,
            title: string(thread.get("title")).unwrap_or_else(|| "Untitled thread".into()),
            status: string(thread.get("status")).unwrap_or_else(|| "Waiting".into()),
            is_pinned: thread.get("pinnedAt").is_some_and(|value| !value.is_null()) || bool(thread.get("isPinned")),
            branch: string(thread.get("branch")),
            latest_user_message_at: string(thread.get("latestUserMessageAt")),
            latest_run_completed_at: string(thread.get("latestRunCompletedAt")),
            active_run_started_at: string(thread.get("activityRunStartedAt")).or_else(|| string(thread.get("activeRunStartedAt"))),
            usage_limit_reset_at: string(thread.get("usageLimitResetAt")),
            id,
            updated_at,
        })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_shell_threads_to_environment_scoped_rows() {
        let rows = decode_shell_snapshot(br#"{
          "projects":[{"id":"p1","name":"Office"}],
          "threads":[{"id":"t1","projectId":"p1","title":"Deploy","status":"running","updatedAt":"2026-11-11T10:00:00Z","pinnedAt":"2026-11-11T09:00:00Z"}]
        }"#).unwrap();
        assert_eq!(rows[0].project_title, "Office");
        assert_eq!(rows[0].id, "t1");
        assert!(rows[0].is_pinned);
    }

    #[test]
    fn rejects_rows_without_identity_or_timestamp() {
        assert_eq!(decode_shell_snapshot(br#"{"projects":[],"threads":[{}]}"#), Err(RemoteSnapshotError::MissingThreadId));
        assert_eq!(decode_shell_snapshot(br#"{"projects":[],"threads":[{"id":"t"}]}"#), Err(RemoteSnapshotError::MissingTimestamp));
    }
}
