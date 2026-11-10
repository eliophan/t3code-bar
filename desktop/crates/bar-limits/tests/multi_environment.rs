use bar_identity::{EnvironmentId, EnvironmentSelection};
use bar_limits::{merge_contributions, LiveLimitsParser};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

#[test]
fn upstream_synthetic_parity() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/multi-environment/limits/parity.json"
    ))
    .unwrap();
    let now: DateTime<Utc> = corpus["now"].as_str().unwrap().parse().unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 10);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let contributions: Vec<_> = case["envs"].as_array().unwrap().iter().map(|env| {
            let mut parser = LiveLimitsParser::default();
            parser.apply(&json!({"version":1,"type":"snapshot","config":{"providers":env["providers"],"usageLimitSources":env["sources"]}})).unwrap();
            parser.contribution(EnvironmentId::new(env["id"].as_str().unwrap()).unwrap())
        }).collect();
        let selection = case["selection"]
            .as_str()
            .map_or(EnvironmentSelection::All, |id| {
                EnvironmentSelection::One(EnvironmentId::new(id).unwrap())
            });
        let snapshot = merge_contributions(&contributions, &selection);
        let mut windows: Vec<Vec<f64>> = snapshot
            .accounts
            .iter()
            .map(|a| a.windows.iter().map(|w| w.used_percent).collect())
            .collect();
        windows.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let mut pools: Vec<Value> = snapshot
            .groups(now)
            .iter()
            .flat_map(|g| {
                g.rows
                    .iter()
                    .map(|r| json!({"driver":g.driver,"id":r.id,"usedPercent":r.used_percent}))
            })
            .collect();
        pools.sort_by_key(|p| {
            format!(
                "{}:{}",
                p["driver"].as_str().unwrap(),
                p["id"].as_str().unwrap()
            )
        });
        let actual = json!({"accounts": snapshot.accounts.len(), "windows":windows, "pools":pools});
        let expected = if case.get("divergence").is_some() {
            assert_eq!(name, "unknown hub identities scoped to environment");
            assert_eq!(case["upstream"]["accounts"], 1);
            json!({"accounts":2,"windows":[[20.0],[40.0]],"pools":[{"driver":"codex","id":"session","usedPercent":30}]})
        } else {
            case["upstream"].clone()
        };
        assert_eq!(actual["accounts"], expected["accounts"], "{name}");
        assert_eq!(actual["pools"], expected["pools"], "{name}");
        let expected_windows: Vec<Vec<f64>> =
            serde_json::from_value(expected["windows"].clone()).unwrap();
        assert_eq!(windows, expected_windows, "{name}");
    }
}
