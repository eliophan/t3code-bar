        SELECT
            t.thread_id,
            t.title,
            t.project_id,
            COALESCE(p.title, 'No Project') as project_title,
            t.created_at,
            t.updated_at,
            (
                SELECT r.status
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status IN ('preparing', 'starting', 'running', 'waiting')
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.ordinal DESC, r.run_id DESC
                LIMIT 1
            ) as active_run_status,
            (
                SELECT COALESCE(
                  json_extract(r.payload_json, '$.workStartedAt'),
                  json_extract(r.payload_json, '$.startedAt'),
                  r.requested_at
                )
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status IN ('preparing', 'starting', 'running', 'waiting')
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.ordinal DESC, r.run_id DESC
                LIMIT 1
            ) as active_run_started_at,
            (
                SELECT r.status
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status != 'queued'
                  AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
                LIMIT 1
            ) as latest_run_status,
            (
                SELECT COALESCE(json_extract(r.payload_json, '$.startedAt'), r.requested_at)
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status != 'queued'
                  AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
                LIMIT 1
            ) as latest_run_started_at,
            (
                SELECT r.completed_at
                FROM orchestration_v2_projection_runs r
                WHERE r.thread_id = t.thread_id
                  AND r.status != 'queued'
                  AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
                  AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
                ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
                LIMIT 1
            ) as latest_run_completed_at,
            (SELECT COUNT(*) FROM orchestration_v2_projection_runtime_requests req WHERE req.thread_id = t.thread_id AND req.status = 'pending' AND req.kind = 'user_input') as pending_user_input,
            (SELECT COUNT(*) FROM orchestration_v2_projection_runtime_requests req WHERE req.thread_id = t.thread_id AND req.status = 'pending' AND req.kind != 'user_input') as pending_approval,
            json_extract(t.payload_json, '$.branch') as branch,
            json_extract(t.payload_json, '$.worktreePath') as worktree_path,
            json_extract(t.payload_json, '$.modelSelection.model') as model_name,
            (SELECT number FROM projection_thread_pull_requests pr WHERE pr.thread_id = t.thread_id LIMIT 1) as pr_number,
            CASE WHEN json_extract(t.payload_json, '$.pinnedAt') IS NOT NULL THEN 1 ELSE 0 END as is_pinned,
            CASE WHEN json_extract(t.payload_json, '$.settledAt') IS NOT NULL THEN 1 ELSE 0 END as is_settled,
            json_extract(t.payload_json, '$.snoozedUntil') as snoozed_until,
            CASE WHEN (SELECT json_extract(pt.payload_json, '$.goal.status') FROM orchestration_v2_projection_provider_threads pt WHERE pt.thread_id = t.thread_id ORDER BY pt.last_run_ordinal DESC LIMIT 1) = 'active' THEN 1 ELSE 0 END as has_goal,
            json_extract(t.payload_json, '$.snoozedAt') as snoozed_at,
            json_extract(t.payload_json, '$.pinOrderKey') as pin_order_key,
            json_extract(t.payload_json, '$.activeOrderKey') as active_order_key,
            (SELECT group_concat(DISTINCT json_extract(r.payload_json, '$.providerInstanceId')) FROM orchestration_v2_projection_runs r WHERE r.thread_id = t.thread_id) as provider_history,
            (SELECT m.updated_at FROM orchestration_v2_projection_messages m WHERE m.thread_id = t.thread_id AND m.role = 'user' ORDER BY m.updated_at DESC LIMIT 1) as latest_user_message_at,
            p.project_icon_json,
            p.favicon_path,
            p.workspace_root,
            json_extract(t.payload_json, '$.unsettledAt') as unsettled_at,
            json_extract(t.payload_json, '$.lastVisitedAt') as last_visited_at,
            (SELECT json_extract(pt.payload_json, '$.status') FROM orchestration_v2_projection_provider_threads pt WHERE pt.thread_id = t.thread_id ORDER BY pt.last_run_ordinal DESC LIMIT 1) as session_status,
            (SELECT json_extract(pt.payload_json, '$.updatedAt') FROM orchestration_v2_projection_provider_threads pt WHERE pt.thread_id = t.thread_id ORDER BY pt.last_run_ordinal DESC LIMIT 1) as session_updated_at,
                (
        SELECT CASE WHEN r.status = 'failed' THEN (
            SELECT json_extract(ti.payload_json, '$.failure')
            FROM orchestration_v2_projection_turn_items ti
            WHERE ti.thread_id = r.thread_id AND ti.run_id = r.run_id
              AND ti.type = 'error' AND ti.status = 'failed'
              AND ti.node_id IS json_extract(r.payload_json, '$.rootNodeId')
            ORDER BY ti.updated_at DESC, ti.ordinal DESC, ti.turn_item_id DESC
            LIMIT 1
        ) END
        FROM orchestration_v2_projection_runs r
        WHERE r.thread_id = t.thread_id
          AND r.status != 'queued'
          AND NOT (r.status = 'cancelled' AND json_extract(r.payload_json, '$.startedAt') IS NULL)
          AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
        ORDER BY r.completed_at IS NULL DESC, r.completed_at DESC, r.ordinal DESC
        LIMIT 1
    ) as latest_failure,
            (SELECT COUNT(*) FROM json_each(t.payload_json, '$.guestShare.messages') g WHERE json_extract(g.value, '$.status') = 'pending') as pending_guest_messages,
            EXISTS (SELECT 1 FROM orchestration_v2_projection_runs r WHERE r.thread_id = t.thread_id) as has_runs,
            COALESCE(    (
        SELECT CASE WHEN r.status = 'running'
            AND EXISTS (
                SELECT 1 FROM orchestration_v2_projection_turn_items ti
                WHERE ti.thread_id = r.thread_id AND ti.run_id = r.run_id
                  AND ti.status IN ('running', 'waiting') AND     ti.type = 'dynamic_tool' AND (
        json_extract(ti.payload_json, '$.toolName') GLOB '*t3_thread_wait'
        OR (json_extract(ti.payload_json, '$.toolName') GLOB '*delegate_task'
            AND json_extract(ti.payload_json, '$.input.mode') = 'wait')
    ))
            AND NOT EXISTS (
                SELECT 1 FROM orchestration_v2_projection_turn_items ti
                WHERE ti.thread_id = r.thread_id AND ti.run_id = r.run_id
                  AND ti.type IN ('command_execution', 'dynamic_tool', 'subagent', 'file_change', 'file_search', 'web_search', 'compaction')
                  AND ti.status IN ('running', 'waiting')
                  AND NOT (ti.type = 'subagent' AND json_extract(ti.payload_json, '$.origin') = 'app_owned')
                  AND NOT (    ti.type = 'dynamic_tool' AND (
        json_extract(ti.payload_json, '$.toolName') GLOB '*t3_thread_wait'
        OR (json_extract(ti.payload_json, '$.toolName') GLOB '*delegate_task'
            AND json_extract(ti.payload_json, '$.input.mode') = 'wait')
    )))
            THEN 1 ELSE 0 END
        FROM orchestration_v2_projection_runs r
        WHERE r.thread_id = t.thread_id
          AND r.status IN ('preparing', 'starting', 'running', 'waiting')
          AND COALESCE(json_extract(r.payload_json, '$.silent'), 0) != 1
        ORDER BY r.ordinal DESC, r.run_id DESC
        LIMIT 1
    ), 0) as blocked_on_thread_wait
        FROM orchestration_v2_projection_threads t
        LEFT JOIN projection_projects p ON t.project_id = p.project_id
        WHERE t.thread_id NOT LIKE 'thread:%' AND t.deleted_at IS NULL AND t.archived_at IS NULL
          AND json_extract(t.payload_json, '$.settledAt') IS NULL
          AND json_extract(t.payload_json, '$.delegatedFrom') IS NULL
          AND COALESCE(json_extract(t.payload_json, '$.lineage.sideChat'), 0) != 1
          AND COALESCE(json_extract(t.payload_json, '$.lineage.relationshipToParent'), '') != 'subagent'
