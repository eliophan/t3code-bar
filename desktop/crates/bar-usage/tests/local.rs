use bar_usage::{LocalRoots, Period, UsageReader};
use chrono::{TimeZone, Utc};
use rusqlite::Connection;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};
#[test]
fn reads_cursor_opencode_json_and_readonly_sqlite() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-local-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let state = dir.join("state");
    let root = dir.join("opencode");
    fs::create_dir_all(root.join("storage/message/s")).unwrap();
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join("usage-scan-cache-v11.json"),
        r#"{"version":11,"sources":{},"models":[],"sessions":[],"files":{}}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    let ms = now.timestamp_millis() - 1000;
    let cursor = state.join("cursor.json");
    fs::write(&cursor,format!(r#"{{"version":1,"strings":["model-thinking","session"],"accounts":{{"a":{{"fetchedAtMs":{ms},"records":[[{ms},0,1,5,3,2,4,0.5,"cursor-id"]]}}}}}}"#)).unwrap();
    fs::write(root.join("storage/message/s/legacy.json"),format!(r#"{{"role":"assistant","sessionID":"legacy-session","modelID":"model","time":{{"created":{ms}}},"tokens":{{"input":10,"output":4,"reasoning":2,"cache":{{"read":3,"write":1}}}}}}"#)).unwrap();
    let db = Connection::open(root.join("opencode.db")).unwrap();
    db.execute_batch(
        "CREATE TABLE message (id TEXT, session_id TEXT, data TEXT, time_created INTEGER)",
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES (?1,?2,?3,?4)",
        rusqlite::params![
            "db-id",
            "db-session",
            r#"{"role":"assistant","modelID":"model","tokens":{"input":7,"output":1}}"#,
            ms
        ],
    )
    .unwrap();
    drop(db);
    let mut reader = UsageReader::new(state.clone());
    reader.set_local_roots(LocalRoots {
        cursor: Some(cursor),
        opencode: vec![root],
        antigravity: vec![],
    });
    let report = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(report.tokens.processed, 14 + 20 + 8);
    assert_eq!(report.tokens.reasoning, 2);
    assert_eq!(report.records, 3);
    assert_eq!(report.sessions, 3);
    assert_eq!(
        report
            .local_sources
            .iter()
            .find(|s| s.kind == "cursor")
            .unwrap()
            .state,
        "ready"
    );
    assert_eq!(
        report
            .local_sources
            .iter()
            .find(|s| s.kind == "opencode")
            .unwrap()
            .state,
        "ready"
    );
    std::fs::write(state.join("usage-host-id"), "verified-host").unwrap();
    let window = bar_usage::Window::new(Period::Day, now, chrono_tz::UTC);
    let query = bar_usage::UsageQuery::for_window(&window, None);
    let contribution = reader.read_contribution(
        &window, None, &bar_usage::LocalProvenance { base_dir: state, ..Default::default() },
        bar_identity::EnvironmentId::new("local").unwrap(), "local".into(), now,
    ).unwrap();
    assert!(contribution.unknown_sources.is_empty());
    let report = bar_usage::merge_usage(&[contribution.contribution], &query, &bar_identity::EnvironmentSelection::All, bar_usage::USAGE_CONTRACT_VERSION).to_report(&window);
    assert_eq!(report.tokens.processed, 14 + 20 + 8);
    fs::remove_dir_all(dir).unwrap();
}
fn varint(mut n: u64) -> Vec<u8> {
    let mut v = Vec::new();
    while n >= 128 {
        v.push(n as u8 | 128);
        n >>= 7;
    }
    v.push(n as u8);
    v
}
fn number(field: u64, n: u64) -> Vec<u8> {
    let mut v = varint(field * 8);
    v.extend(varint(n));
    v
}
fn bytes(field: u64, b: &[u8]) -> Vec<u8> {
    let mut v = varint(field * 8 + 2);
    v.extend(varint(b.len() as u64));
    v.extend(b);
    v
}
#[test]
fn antigravity_merges_shared_usage_across_databases() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-antigravity-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let state = dir.join("state");
    let root = dir.join("antigravity/conversations");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(
        state.join("usage-scan-cache-v11.json"),
        r#"{"version":11,"sources":{},"models":[],"sessions":[],"files":{}}"#,
    )
    .unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    for (name, input) in [("a.db", 10), ("b.db", 20)] {
        let db = Connection::open(root.join(name)).unwrap();
        db.execute_batch("CREATE TABLE steps (idx INTEGER, metadata BLOB)")
            .unwrap();
        let mut usage = number(2, input);
        usage.extend(number(3, 5));
        usage.extend(bytes(11, b"same"));
        let model = bytes(12, b"Gemini 3 Flash Preview");
        let time = number(1, (now.timestamp() - 10) as u64);
        let mut step = bytes(9, &usage);
        step.extend(bytes(24, &model));
        step.extend(bytes(8, &time));
        db.execute("INSERT INTO steps VALUES (1,?1)", [step])
            .unwrap();
    }
    let mut reader = UsageReader::new(state);
    reader.set_local_roots(LocalRoots {
        cursor: None,
        opencode: vec![],
        antigravity: vec![root.parent().unwrap().to_path_buf()],
    });
    let report = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(report.records, 1);
    assert_eq!(report.tokens.processed, 25);
    assert_eq!(
        report
            .local_sources
            .iter()
            .find(|s| s.kind == "antigravity")
            .unwrap()
            .state,
        "ready"
    );
    assert_eq!(report.models[0].name, "gemini-3-flash-preview");
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn local_data_works_without_cache_and_survives_unreadable_refresh() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-retain-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    let ms = now.timestamp_millis() - 1000;
    let cursor = dir.join("cursor.json");
    fs::write(&cursor,format!(r#"{{"version":1,"strings":["model","session"],"accounts":{{"a":{{"fetchedAtMs":{ms},"records":[[{ms},0,1,5,3,2,4,0.5,"id"]]}}}}}}"#)).unwrap();
    let mut reader = UsageReader::new(dir.clone());
    reader.set_local_roots(LocalRoots {
        cursor: Some(cursor.clone()),
        ..LocalRoots::default()
    });
    let first = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert!(first.scan_cache_missing);
    assert_eq!(first.tokens.processed, 14);
    fs::write(cursor, "{").unwrap();
    let second = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(second.tokens.processed, 14);
    assert_eq!(
        second
            .local_sources
            .iter()
            .find(|s| s.kind == "cursor")
            .unwrap()
            .state,
        "partial"
    );
    fs::remove_dir_all(dir).unwrap();
}
#[test]
fn truncated_cache_reports_unreadable_and_keeps_last_good_report() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-cache-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let cache = dir.join("usage-scan-cache-v11.json");
    fs::write(&cache, r#"{"version":11,"files":{}}"#).unwrap();
    let mut reader = UsageReader::new(dir.clone());
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    fs::write(cache, r#"{"version":11,"files":{"#).unwrap();
    assert!(matches!(
        reader.read(Period::Day, now, chrono_tz::UTC),
        Err(bar_usage::UsageError::Unreadable)
    ));
    assert!(reader.last_good().is_some());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_refresh_keeps_existing_files_but_prunes_deleted_files() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-prune-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let root = dir.join("storage/message/s");
    fs::create_dir_all(&root).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    let message = format!(
        r#"{{"role":"assistant","modelID":"model","time":{{"created":{}}},"tokens":{{"input":10}}}}"#,
        now.timestamp_millis() - 1000
    );
    fs::write(root.join("a.json"), &message).unwrap();
    fs::write(root.join("b.json"), &message).unwrap();
    let mut reader = UsageReader::new(dir.join("state"));
    reader.set_local_roots(LocalRoots {
        opencode: vec![dir.clone()],
        ..LocalRoots::default()
    });
    assert_eq!(
        reader
            .read(Period::Day, now, chrono_tz::UTC)
            .unwrap()
            .tokens
            .processed,
        20
    );
    fs::remove_file(root.join("a.json")).unwrap();
    fs::write(root.join("b.json"), "{").unwrap();
    let report = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(report.tokens.processed, 10);
    fs::write(root.join("b.json"), message.replace("\"assistant\"", "3")).unwrap();
    let report = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(report.tokens.processed, 10);
    assert_eq!(
        report
            .local_sources
            .iter()
            .find(|s| s.kind == "opencode")
            .unwrap()
            .state,
        "partial"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn discovery_obeys_environment_and_finds_antigravity_instances() {
    use std::collections::HashMap;
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-discover-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let state = dir.join("state");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("settings.json"), r#"{"providerInstances":{"instance":{"driver":"antigravity","environment":{"TOKEN":"private"}},"other":{"driver":"opencode"}}}"#).unwrap();
    let env = HashMap::from([
        ("OPENCODE_DATA_DIR".into(), " ~/oc-a, /tmp/oc-b ".into()),
        ("ANTIGRAVITY_DATA_DIR".into(), "/tmp/ag".into()),
        ("XDG_DATA_HOME".into(), "/ignored".into()),
    ]);
    let roots = LocalRoots::discover_with_environment(&state, &dir, &env);
    assert_eq!(roots.opencode, vec![dir.join("oc-a"), "/tmp/oc-b".into()]);
    assert_eq!(roots.antigravity[0], std::path::PathBuf::from("/tmp/ag"));
    assert_eq!(roots.antigravity[1], state.join("providers/antigravity/5e8c03a94c7835b87dd82c690f9f85c08592ef7433aa141ceaacb36a9f4da434/antigravity-acp"));
    assert_eq!(
        roots.cursor,
        Some(state.join("usage-cursor-account-cache-v1.json"))
    );
    let defaults = LocalRoots::discover_with_environment(
        &state,
        &dir,
        &HashMap::from([("XDG_DATA_HOME".into(), "relative".into())]),
    );
    assert_eq!(defaults.opencode, vec![dir.join(".local/share/opencode")]);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sqlite_bad_cells_and_typed_optional_fields_are_counted_per_row() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-cells-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    let db = Connection::open(dir.join("opencode.db")).unwrap();
    db.execute_batch("CREATE TABLE message (id, session_id, data, time_created)")
        .unwrap();
    let valid = r#"{"role":null,"modelID":"model","tokens":{"input":10}}"#;
    db.execute(
        "INSERT INTO message VALUES ('good','s',?1,?2)",
        rusqlite::params![valid, now.timestamp_millis() - 1000],
    )
    .unwrap();
    db.execute(
        "INSERT INTO message VALUES ('bad','s',3,?1)",
        [now.timestamp_millis() - 1000],
    )
    .unwrap();
    for (i, invalid) in [
        r#"{"role":"user","model":3}"#,
        r#"{"modelID":"model","time":3}"#,
        r#"{"modelID":"model","tokens":{"cache":3}}"#,
        r#"{"modelID":"model","cost":"3"}"#,
    ]
    .iter()
    .enumerate()
    {
        db.execute(
            "INSERT INTO message VALUES (?1,'s',?2,?3)",
            rusqlite::params![format!("bad{i}"), invalid, now.timestamp_millis() - 1000],
        )
        .unwrap();
    }
    drop(db);
    let mut reader = UsageReader::new(dir.join("state"));
    reader.set_local_roots(LocalRoots {
        opencode: vec![dir.clone()],
        ..LocalRoots::default()
    });
    let report = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(report.tokens.processed, 10);
    assert_eq!(report.malformed_records, 5);
    let status = report
        .local_sources
        .iter()
        .find(|s| s.kind == "opencode")
        .unwrap();
    assert_eq!(status.state, "partial");
    assert_eq!(status.data_updated_at, None);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn antigravity_retains_unmerged_file_and_prunes_deleted_file() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-ag-retain-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    for (name, input) in [("a.db", 20), ("b.db", 10)] {
        let db = Connection::open(dir.join(name)).unwrap();
        db.execute_batch("CREATE TABLE steps (idx, metadata)")
            .unwrap();
        let mut usage = number(2, input);
        usage.extend(bytes(11, b"shared"));
        let mut step = bytes(9, &usage);
        step.extend(bytes(8, &number(1, (now.timestamp() - 10) as u64)));
        db.execute("INSERT INTO steps VALUES (1,?1)", [step])
            .unwrap();
        if name == "b.db" {
            db.execute("INSERT INTO steps VALUES (2,3)", []).unwrap();
        }
    }
    let mut reader = UsageReader::new(dir.join("state"));
    reader.set_local_roots(LocalRoots {
        antigravity: vec![dir.clone()],
        ..LocalRoots::default()
    });
    let first = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(first.tokens.processed, 20);
    assert_eq!(first.malformed_records, 1);
    fs::remove_file(dir.join("a.db")).unwrap();
    fs::write(dir.join("b.db"), b"corrupt").unwrap();
    let second = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(second.tokens.processed, 10);
    let status = second
        .local_sources
        .iter()
        .find(|s| s.kind == "antigravity")
        .unwrap();
    assert_eq!(status.state, "partial");
    assert_eq!(status.failed_files, 1);
    assert_eq!(status.data_updated_at, None);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn opencode_wal_refreshes_without_changing_main_database() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-wal-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let db = Connection::open(dir.join("opencode.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE message (id, session_id, data, time_created)").unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    let json = r#"{"modelID":"model","tokens":{"input":10}}"#;
    db.execute(
        "INSERT INTO message VALUES ('one','s',?1,?2)",
        rusqlite::params![json, now.timestamp_millis() - 1000],
    )
    .unwrap();
    let mut reader = UsageReader::new(dir.join("state"));
    reader.set_local_roots(LocalRoots {
        opencode: vec![dir.clone()],
        ..LocalRoots::default()
    });
    assert_eq!(
        reader
            .read(Period::Day, now, chrono_tz::UTC)
            .unwrap()
            .tokens
            .processed,
        10
    );
    let main = fs::read(dir.join("opencode.db")).unwrap();
    db.execute(
        "INSERT INTO message VALUES ('two','s',?1,?2)",
        rusqlite::params![json, now.timestamp_millis() - 1000],
    )
    .unwrap();
    assert_eq!(fs::read(dir.join("opencode.db")).unwrap(), main);
    assert_eq!(
        reader
            .read(Period::Day, now, chrono_tz::UTC)
            .unwrap()
            .tokens
            .processed,
        20
    );
    drop(db);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_json_reports_modification_time_and_null_optional_fields() {
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-json-time-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let messages = dir.join("storage/message/s");
    fs::create_dir_all(&messages).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 9, 8, 0).unwrap();
    let path = messages.join("m.json");
    fs::write(&path, format!(r#"{{"role":null,"model":null,"modelID":"model","time":{{"created":{}}},"tokens":{{"input":10,"cache":null}},"cost":null}}"#, now.timestamp_millis() - 1000)).unwrap();
    let modified = chrono::DateTime::<Utc>::from(fs::metadata(&path).unwrap().modified().unwrap());
    let mut reader = UsageReader::new(dir.join("state"));
    reader.set_local_roots(LocalRoots {
        opencode: vec![dir.clone()],
        ..LocalRoots::default()
    });
    let report = reader.read(Period::Day, now, chrono_tz::UTC).unwrap();
    assert_eq!(report.tokens.processed, 10);
    let status = report
        .local_sources
        .iter()
        .find(|s| s.kind == "opencode")
        .unwrap();
    assert_eq!(status.data_updated_at, Some(modified));
    fs::remove_dir_all(dir).unwrap();
}
