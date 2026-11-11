use bar_threads::*;
use chrono::{TimeZone, Utc};
use rusqlite::Connection;
use std::{fs, path::PathBuf};
use tempfile::TempDir;
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}
fn fixture() -> (TempDir, Configuration) {
    let dir = TempDir::new().unwrap();
    let db = Connection::open(dir.path().join("statev2.sqlite")).unwrap();
    db.execute_batch(&fs::read_to_string(root().join("threads.sql")).unwrap())
        .unwrap();
    let c = Configuration::from_environment(Some(dir.path().to_owned()));
    (dir, c)
}
fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap()
}
#[test]
fn optional_nontext_dates_do_not_break_snapshot() {
    let (_dir, c) = fixture();
    let db = Connection::open(c.base_dir.join("statev2.sqlite")).unwrap();
    db.execute("UPDATE orchestration_v2_projection_threads SET payload_json=json_set(payload_json,'$.snoozedUntil',json('false'),'$.lastVisitedAt',json('false')) WHERE thread_id='approval'", []).unwrap();
    let snapshot = read_threads(&c, now()).unwrap();
    let thread = snapshot
        .threads
        .iter()
        .find(|t| t.id == "approval")
        .unwrap();
    assert_eq!(thread.status, ThreadStatus::Approval);
    assert!(thread.last_visited_at.is_none());
}
#[test]
fn swift_iso8601_offsets_preserve_required_dates_and_snoozes() {
    let (_dir, c) = fixture();
    let db = Connection::open(c.base_dir.join("statev2.sqlite")).unwrap();
    db.execute("UPDATE orchestration_v2_projection_threads SET created_at='2026-11-04T00:00:00+0000', payload_json=json_set(payload_json,'$.snoozedUntil','2099-01-01T00:00:00+0000') WHERE thread_id='input'", []).unwrap();
    db.execute("UPDATE orchestration_v2_projection_runtime_requests SET status='resolved' WHERE thread_id='input'", []).unwrap();
    let snapshot = read_threads(&c, now()).unwrap();
    assert!(!snapshot.threads.iter().any(|t| t.id == "input"));
}
#[test]
fn frozen_swift_export_matches() {
    let (_dir, c) = fixture();
    let mut s = read_threads(&c, now()).unwrap();
    s.summary = ThreadSummary::new(&s.threads, true);
    let preview = read_preview(&c, "approval").unwrap();
    let actual = export_json(&s, preview.as_ref());
    let expected: serde_json::Value =
        serde_json::from_slice(&fs::read(root().join("expected/threads.json")).unwrap()).unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn fallback_and_missing_schema() {
    let (_dir, c) = fixture();
    let db = Connection::open(c.base_dir.join("statev2.sqlite")).unwrap();
    db.execute_batch("DROP TABLE orchestration_v2_projection_turn_items")
        .unwrap();
    let s = read_threads(&c, now()).unwrap();
    assert_eq!(
        s.threads.iter().find(|t| t.id == "limited").unwrap().status,
        ThreadStatus::Failed
    );
    assert_eq!(
        s.threads.iter().find(|t| t.id == "waiting").unwrap().status,
        ThreadStatus::Working
    );
    db.execute_batch("DROP TABLE orchestration_v2_projection_threads")
        .unwrap();
    assert!(matches!(
        read_threads(&c, now()),
        Err(DomainError::IncompatibleSchema)
    ));
}
#[test]
fn hidden_snooze_and_transitions() {
    let (_dir, c) = fixture();
    let db = Connection::open(c.base_dir.join("statev2.sqlite")).unwrap();
    let ids = |c: &Configuration| {
        read_threads(c, now())
            .unwrap()
            .threads
            .into_iter()
            .map(|t| t.id)
            .collect::<Vec<_>>()
    };
    let initial = ids(&c);
    assert!(!initial.contains(&"snoozed".into()));
    db.execute("UPDATE orchestration_v2_projection_threads SET payload_json=json_set(payload_json,'$.snoozedUntil','2099-01-01T00:00:00Z','$.snoozedAt','2026-11-06T09:00:00Z') WHERE thread_id='approval'",[]).unwrap();
    assert!(ids(&c).contains(&"approval".into()));
    db.execute("UPDATE orchestration_v2_projection_runtime_requests SET status='resolved' WHERE thread_id='approval'",[]).unwrap();
    assert!(!ids(&c).contains(&"approval".into()));
    let offline = ThreadSummary::new(&read_threads(&c, now()).unwrap().threads, false);
    assert_eq!(
        (offline.status, offline.badge_count, offline.attention_count),
        (ThreadStatus::Settled, 0, 0)
    );
}
#[test]
fn preview_and_url() {
    let (_dir, c) = fixture();
    assert_eq!(
        preview_excerpt("## Header\n**bold** [link](https://x)\n```\nsecret\n```\n- one\n> quote"),
        "Header\nbold link\n• one\nquote"
    );
    assert_eq!(preview_excerpt("a\nb\nc\nd\ne\nf"), "a\nb\nc\nd\ne…");
    assert!(preview_excerpt(&"word ".repeat(200)).graphemes_count() <= 361);
    fs::write(c.base_dir.join("environment-id"), "env /%#\n").unwrap();
    fs::write(
        c.base_dir.join("server-runtime.json"),
        r#"{"origin":"http://evil.example:4567"}"#,
    )
    .unwrap();
    assert_eq!(
        c.thread_url("a/b%# é", true).unwrap(),
        "http://127.0.0.1:4567/env%20%2F%25%23/a%2Fb%25%23%20%C3%A9"
    );
    assert_eq!(
        c.thread_url("a/b", false).unwrap(),
        "t3code://threads/env%20%2F%25%23/a%2Fb"
    );
}
trait Count {
    fn graphemes_count(&self) -> usize;
}
impl Count for String {
    fn graphemes_count(&self) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        self.graphemes(true).count()
    }
}
#[test]
fn missing_db_and_provider() {
    let d = TempDir::new().unwrap();
    let c = Configuration::from_environment(Some(d.path().into()));
    assert!(matches!(read_threads(&c, now()), Err(DomainError::Missing)));
    assert!(matches!(read_preview(&c, "x"), Err(DomainError::Missing)));
    assert!(!c.base_dir.join("statev2.sqlite").exists());
    assert_eq!(ProviderKind::resolve("pipeline"), ProviderKind::Unknown);
    assert_eq!(ProviderKind::resolve("claudeAgent_1"), ProviderKind::Claude);
}
#[test]
fn status_and_order_changes_are_observed() {
    let (_dir, c) = fixture();
    let db = Connection::open(c.base_dir.join("statev2.sqlite")).unwrap();
    let get = |id: &str| {
        read_threads(&c, now())
            .unwrap()
            .threads
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    };
    assert_eq!(get("done").status, ThreadStatus::Done);
    db.execute("UPDATE orchestration_v2_projection_threads SET payload_json=json_set(payload_json,'$.lastVisitedAt','2099-01-01T00:00:00Z') WHERE thread_id='done'",[]).unwrap();
    assert_eq!(get("done").status, ThreadStatus::Settled);
    db.execute("UPDATE orchestration_v2_projection_threads SET payload_json=json_set(payload_json,'$.snoozedUntil','2020-01-01T00:00:00Z') WHERE thread_id='request'",[]).unwrap();
    assert_eq!(get("request").status, ThreadStatus::Request);
    let s = read_threads(&c, now()).unwrap();
    let tie: Vec<_> = s
        .threads
        .into_iter()
        .filter(|t| t.status == ThreadStatus::Input || t.status == ThreadStatus::Request)
        .collect();
    let summary = ThreadSummary::new(&tie, true);
    assert_eq!(summary.status, ThreadStatus::Input);
    assert_eq!(summary.badge_count, 2);
}
#[test]
fn malformed_data_and_read_only_database() {
    let (_dir, c) = fixture();
    let p = c.base_dir.join("statev2.sqlite");
    let before = fs::read(&p).unwrap();
    read_threads(&c, now()).unwrap();
    read_preview(&c, "approval").unwrap();
    assert_eq!(before, fs::read(&p).unwrap());
    let db = Connection::open(&p).unwrap();
    db.execute("UPDATE orchestration_v2_projection_threads SET created_at='bad' WHERE thread_id='approval'",[]).unwrap();
    assert!(matches!(
        read_threads(&c, now()),
        Err(DomainError::InvalidData)
    ));
}
#[test]
fn sql_excludes_all_hidden_shapes() {
    let (_dir, c) = fixture();
    let db = Connection::open(c.base_dir.join("statev2.sqlite")).unwrap();
    for (id, payload, deleted, archived) in [
        ("thread:internal", "{}", None, None),
        ("deleted", "{}", Some("yes"), None),
        ("archived-extra", "{}", None, Some("yes")),
        ("settled-extra", r#"{"settledAt":"x"}"#, None, None),
        ("side-extra", r#"{"lineage":{"sideChat":true}}"#, None, None),
        (
            "child-extra",
            r#"{"lineage":{"relationshipToParent":"subagent"}}"#,
            None,
            None,
        ),
    ] {
        db.execute("INSERT INTO orchestration_v2_projection_threads VALUES (?1,'hidden','fixture-project','2026-11-04T00:00:00Z','2026-11-04T00:00:00Z',?2,'default',?3,?4)",rusqlite::params![id,payload,deleted,archived]).unwrap();
    }
    let s = read_threads(&c, now()).unwrap();
    assert_eq!(s.threads.len(), 7);
}

#[test]
fn sidebar_visibility_is_consistent_in_both_schemas() {
    for fallback in [false, true] {
        let (_dir, c) = fixture();
        let db = Connection::open(c.base_dir.join("statev2.sqlite")).unwrap();
        if fallback {
            db.execute_batch("DROP TABLE orchestration_v2_projection_turn_items")
                .unwrap();
        }
        for (id, payload) in [
            ("main-missing-lineage", r#"{"createdBy":"user"}"#),
            ("settled-override", r#"{"settledOverride":"settled"}"#),
            ("main-null-delegation", r#"{"delegatedFrom":null}"#),
            (
                "main-agent-created",
                r#"{"createdBy":"agent","creationSource":"mcp"}"#,
            ),
            (
                "delegated-no-lineage",
                r#"{"delegatedFrom":{"threadId":"parent","environmentId":"local","title":"Parent"}}"#,
            ),
            (
                "delegated-null-lineage",
                r#"{"delegatedFrom":{"threadId":"parent"},"lineage":{"parentThreadId":null,"relationshipToParent":null,"rootThreadId":null},"pinnedAt":"2026-11-04T00:00:00Z"}"#,
            ),
        ] {
            db.execute("INSERT INTO orchestration_v2_projection_threads VALUES (?1,'fixture','fixture-project','2026-11-04T00:00:00Z','2026-11-04T00:00:00Z',?2,'default',NULL,NULL)",rusqlite::params![id,payload]).unwrap();
        }
        let snapshot = read_threads(&c, now()).unwrap();
        let ids: Vec<_> = snapshot.threads.iter().map(|t| t.id.as_str()).collect();
        for id in [
            "main-missing-lineage",
            "main-null-delegation",
            "main-agent-created",
        ] {
            assert!(
                ids.contains(&id),
                "main thread {id} missing (fallback={fallback})"
            );
        }
        for id in ["delegated-no-lineage", "delegated-null-lineage", "settled-override"] {
            assert!(
                !ids.contains(&id),
                "hidden thread {id} visible (fallback={fallback})"
            );
        }
    }
}

#[test]
fn connection_port_uses_runtime_precedence_and_rejects_invalid_values() {
    let (_dir, config) = fixture();
    for (json, expected) in [
        (r#"{"port":1234,"origin":"http://host:5678"}"#, Some(1234)),
        (r#"{"origin":"http://host:5678"}"#, Some(5678)),
        (r#"{"port":0,"origin":"http://host:5678"}"#, None),
        (r#"{"port":65536}"#, None),
    ] {
        fs::write(config.base_dir.join("server-runtime.json"), json).unwrap();
        assert_eq!(config.server_port().ok(), expected);
    }
}

#[test]
fn environment_registry_preserves_duplicate_labels_by_id() {
    let (dir, _config) = fixture();
    let records = vec![
        environments::EnvironmentRecord { environment_id: "server".into(), label: "omarchy".into(), machine: "server".into(), endpoint: "https://relay.example/server".into(), enabled: true },
        environments::EnvironmentRecord { environment_id: "mini".into(), label: "omarchy".into(), machine: "mac-mini".into(), endpoint: "https://relay.example/mini".into(), enabled: true },
    ];
    environments::save(&dir.path().join("bar-environments.json"), &records).unwrap();
    let catalog = environments::load(&dir.path().join("bar-environments.json")).unwrap();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0].environment_id, "server");
    assert_eq!(catalog[1].label, "omarchy");
}

#[test]
fn malformed_or_missing_environment_registry_is_fail_closed() {
    let (_dir, config) = fixture();
    assert!(environments::load(&config.base_dir.join("bar-environments.json")).unwrap().is_empty());
    fs::write(config.base_dir.join("bar-environments.json"), "not json")
    .unwrap();
    assert!(environments::load(&config.base_dir.join("bar-environments.json")).is_err());
}

#[test]
fn environment_registry_rejects_duplicate_ids_and_non_http_endpoints() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bar-environments.json");
    let duplicate = vec![
        environments::EnvironmentRecord { environment_id: "same".into(), label: "one".into(), machine: "server".into(), endpoint: "https://example.test".into(), enabled: true },
        environments::EnvironmentRecord { environment_id: "same".into(), label: "two".into(), machine: "server".into(), endpoint: "https://example.test".into(), enabled: true },
    ];
    assert!(environments::save(&path, &duplicate).is_err());
    let invalid = environments::EnvironmentRecord { environment_id: "one".into(), label: "one".into(), machine: "server".into(), endpoint: "ssh://example.test".into(), enabled: true };
    assert!(environments::save(&path, &[invalid]).is_err());
}

#[test]
fn local_environment_record_has_stable_loopback_route() {
    let record = environments::EnvironmentRecord::local("env-local", "This Mac", 3777);
    assert_eq!(record.environment_id, "env-local");
    assert_eq!(record.endpoint, "http://127.0.0.1:3777");
    assert!(record.enabled);
}

#[test]
fn registry_updates_only_the_selected_environment_status() {
    struct Reachable;
    impl environments::EnvironmentTransport for Reachable {
        fn connect(&self, record: &environments::EnvironmentRecord) -> Result<(), environments::TransportError> {
            if record.environment_id == "server" { Ok(()) } else { Err(environments::TransportError::Unreachable) }
        }
        fn disconnect(&self, _environment_id: &str) {}
    }
    let records = vec![
        environments::EnvironmentRecord { environment_id: "server".into(), label: "omarchy".into(), machine: "server".into(), endpoint: "https://example.test/server".into(), enabled: true },
        environments::EnvironmentRecord { environment_id: "mini".into(), label: "omarchy".into(), machine: "mac-mini".into(), endpoint: "https://example.test/mini".into(), enabled: true },
    ];
    let mut registry = environments::EnvironmentRegistry::new(records).unwrap();
    registry.attach("server", Reachable).unwrap();
    registry.attach("mini", Reachable).unwrap();
    registry.connect("server").unwrap();
    assert_eq!(registry.status("server"), Some(environments::EnvironmentStatus::Reachable));
    assert_eq!(registry.status("mini"), Some(environments::EnvironmentStatus::Disconnected));
    assert_eq!(registry.connect("mini"), Err(environments::TransportError::Unreachable));
    assert_eq!(registry.status("mini"), Some(environments::EnvironmentStatus::Offline));
}

#[test]
fn unsupported_remote_transport_is_explicit() {
    let record = environments::EnvironmentRecord { environment_id: "remote".into(), label: "omarchy".into(), machine: "server".into(), endpoint: "https://relay.example/remote".into(), enabled: true };
    let mut registry = environments::EnvironmentRegistry::new(vec![record]).unwrap();
    registry.attach("remote", environments::UnsupportedRemoteTransport).unwrap();
    let result = registry.connect("remote");
    assert_eq!(result, Err(environments::TransportError::Unsupported));
    assert_eq!(registry.status("remote"), Some(environments::EnvironmentStatus::Unsupported));
}
