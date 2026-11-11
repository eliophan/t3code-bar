use bar_identity::EnvironmentSelection;
use bar_usage::{merge_usage, EnvironmentUsage, Period, UsageContribution, UsageQuery, Window};
use chrono::{TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Case {
    name: String,
    environments: Vec<EnvironmentUsage>,
    selection: EnvironmentSelection,
    expected: Value,
}

#[test]
fn matches_upstream_synthetic_corpus() {
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/multi-environment/usage/parity.json"
    ))
    .unwrap();
    assert!(cases.len() >= 16);
    let window = Window::new(
        Period::Month,
        Utc.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap(),
        chrono_tz::UTC,
    );
    for case in cases {
        let mut query = query();
        query.model_filter = case
            .environments
            .first()
            .and_then(|e| e.summary.model_filter.clone());
        let contributions: Vec<_> = case
            .environments
            .into_iter()
            .map(|e| {
                let mut request = query.clone();
                request.model_filter = e.summary.model_filter.clone();
                UsageContribution::bind(request, e).unwrap()
            })
            .collect();
        let merged = merge_usage(&contributions, &query, &case.selection, 6);
        let report = merged.to_report(&window);
        let actual = json!({
            "costUsd": report.cost_usd, "tokens": report.tokens, "sessions": report.sessions,
            "records": report.records, "unpricedRecords": report.unpriced_records,
            "costByType": report.cost_by_type, "costBySpeed": report.cost_by_speed,
            "cacheSavingsUsd": report.cache_savings_usd, "costQuality": merged.cost_quality(), "models": report.models, "duplicateSources": merged.duplicate_sources,
            "contributingEnvironments": merged.contributing_environments, "contractMismatches": merged.contract_mismatches,
        });
        compare(&actual, &case.expected, &case.name);
    }
}

fn compare(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert!(
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9,
            "{path}: {a} != {b}"
        ),
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (key, value) in b {
                compare(&a[key], value, &format!("{path}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                compare(x, y, &format!("{path}[{i}]"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

#[test]
fn report_places_hourly_and_daily_buckets_in_the_explicit_window() {
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/multi-environment/usage/parity.json"
    ))
    .unwrap();
    let case = cases.into_iter().find(|c| c.name == "quality").unwrap();
    let query = query();
    let contributions: Vec<_> = case
        .environments
        .into_iter()
        .map(|e| UsageContribution::bind(query.clone(), e).unwrap())
        .collect();
    let merged = merge_usage(&contributions, &query, &case.selection, 6);
    let hourly = Window::new(
        Period::Day,
        Utc.with_ymd_and_hms(2026, 9, 3, 11, 0, 0).unwrap(),
        chrono_tz::UTC,
    );
    let report = merged.to_report(&hourly);
    assert_eq!(report.timeline.resolution, "hour");
    assert_eq!(report.timeline.columns[23].by_provider["claude"].tokens, 50);
    assert_eq!(
        report.timeline.columns[23].by_provider["claude"].cost_usd,
        10.0
    );
    let daily = Window::new(
        Period::Week,
        Utc.with_ymd_and_hms(2026, 9, 5, 11, 0, 0).unwrap(),
        chrono_tz::UTC,
    );
    let report = merged.to_report(&daily);
    assert_eq!(report.timeline.resolution, "day");
    assert_eq!(report.timeline.columns[4].by_provider["claude"].tokens, 50);
}

fn query() -> UsageQuery {
    UsageQuery {
        since_day: "2026-09-01".into(),
        until_day: "2026-09-30".into(),
        time_zone: "UTC".into(),
        resolution: "day".into(),
        since_time: None,
        until_time: None,
        model_filter: None,
    }
}

#[test]
fn local_reader_requires_provenance_and_live_response_replaces_fallback() {
    use bar_identity::EnvironmentId;
    use bar_usage::{LocalProvenance, UsageReader};
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-contribution-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let ms = now.timestamp_millis() - 1000;
    let data = json!({"version":11,"sources":{"claude\u{0}/synthetic":{"dir":"/synthetic","volumeId":"1:2"}},"models":["synthetic"],"sessions":["s"],"files":{"/synthetic/file":{"p":"claude","m":ms,"r":[[ms,0,0,10,20,5,15,7,null,10,0]]}}});
    std::fs::write(
        dir.join("usage-scan-cache-v11.json"),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    let window = Window::new(Period::Month, now, chrono_tz::UTC);
    let query = UsageQuery::for_window(&window, None);
    let id = EnvironmentId::new("local").unwrap();
    let mut reader = UsageReader::new(dir.clone());
    let provenance = LocalProvenance {
        base_dir: dir.clone(),
        ..Default::default()
    };
    let unknown = reader
        .read_contribution(&window, None, &provenance, id.clone(), "local".into(), now)
        .unwrap();
    assert_eq!(
        unknown.unknown_sources,
        [("claude".into(), "/synthetic".into())]
    );
    assert_eq!(
        merge_usage(
            &[unknown.contribution],
            &query,
            &EnvironmentSelection::All,
            6
        )
        .to_report(&window)
        .records,
        0
    );
    assert!(!dir.join("usage-host-id").exists());
    std::fs::write(dir.join("usage-host-id"), "verified-host\n").unwrap();
    let local = reader
        .read_contribution(&window, None, &provenance, id.clone(), "local".into(), now)
        .unwrap();
    assert!(local.unknown_sources.is_empty());
    let local_report = merge_usage(
        &[local.contribution.clone()],
        &query,
        &EnvironmentSelection::All,
        6,
    )
    .to_report(&window);
    assert_eq!(local_report.tokens.processed, 50);
    assert_eq!(local_report.cost_usd, 10.0);
    assert_eq!(local_report.sessions, 1);
    let mut live: EnvironmentUsage = serde_json::from_value(json!({"environmentId":"local","label":"local","summary":{"contractVersion":6,"readAt":"2026-09-01T00:00:00Z","timeZone":query.time_zone,"sinceDay":query.since_day,"untilDay":query.until_day,"buckets":[],"sources":[],"pricing":{"status":"fresh","source":"test","fetchedAt":null,"knownModels":0},"scanDurationMs":0}})).unwrap();
    let authoritative = UsageContribution::bind(query.clone(), live.clone()).unwrap();
    let merged = merge_usage(
        &[local.contribution.clone(), authoritative],
        &query,
        &EnvironmentSelection::All,
        6,
    );
    assert_eq!(merged.to_report(&window).records, 0);
    live.summary.time_zone = "Asia/Tokyo".into();
    assert!(UsageContribution::bind(query.clone(), live).is_err());
    let other_query = UsageQuery::for_window(&Window::new(Period::Week, now, chrono_tz::UTC), None);
    assert_eq!(
        merge_usage(
            &[local.contribution],
            &other_query,
            &EnvironmentSelection::All,
            6
        )
        .to_report(&window)
        .records,
        0
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn local_filter_precedes_deduplication_and_uses_original_model() {
    use bar_identity::EnvironmentId;
    use bar_usage::{LocalProvenance, UsageModelFilter, UsageModelSelector, UsageReader};
    let dir = std::env::temp_dir().join(format!(
        "bar-usage-filter-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap();
    let ms = now.timestamp_millis() - 1000;
    let data = json!({"version":11,"sources":{"claude\u{0}/synthetic":{"dir":"/synthetic","volumeId":"1:2"}},"models":["other","wanted"],"sessions":["s"],"files":{"/synthetic/file":{"p":"claude","m":ms,"r":[[ms,0,0,3,0,0,0,0,"same",1,0],[ms,1,0,7,0,0,0,0,"same",2,0]]}}});
    std::fs::write(
        dir.join("usage-scan-cache-v11.json"),
        serde_json::to_vec(&data).unwrap(),
    )
    .unwrap();
    let window = Window::new(Period::Week, now, chrono_tz::UTC);
    let filter = Some(UsageModelFilter {
        include: Some(vec![UsageModelSelector {
            provider: None,
            model: "wanted".into(),
        }]),
        exclude: None,
    });
    let query = UsageQuery::for_window(&window, filter.clone());
    let provenance = LocalProvenance {
        base_dir: dir.clone(),
        host_id_override: Some("host".into()),
        ..Default::default()
    };
    let mut reader = UsageReader::new(dir.clone());
    let local = reader
        .read_contribution(
            &window,
            filter,
            &provenance,
            EnvironmentId::new("local").unwrap(),
            "local".into(),
            now,
        )
        .unwrap();
    let report = merge_usage(&[local.contribution], &query, &EnvironmentSelection::All, 6)
        .to_report(&window);
    assert_eq!(report.tokens.processed, 7);
    assert_eq!(report.cost_usd, 2.0);
    std::fs::remove_dir_all(dir).unwrap();
}
