//! Derived from T3 Code usageMerge.ts at 6647b0a761ff7a1287047a9bb5fd5b63772f456b.
//! See T3-CODE-LICENSE for the upstream MIT license.
use bar_identity::{EnvironmentId, EnvironmentSelection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const USAGE_CONTRACT_VERSION: u32 = 6;
pub const USAGE_MERGE_COMPATIBLE_SINCE: u32 = 4;
/// English ICU collation pins ownership where JavaScript's default locale is environment-dependent.
pub const USAGE_COLLATION_LOCALE: &str = "en";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageTokenTotals {
    pub uncached_input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_creation_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
}
impl UsageTokenTotals {
    pub fn total(&self) -> i64 {
        self.uncached_input_tokens
            + self.cached_input_tokens
            + self.cache_creation_tokens
            + self.output_tokens
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageCategoryCost {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub output: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucket {
    pub day: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour_start: Option<String>,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    pub totals: UsageTokenTotals,
    pub cost_usd: f64,
    pub cache_savings_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_cost_usd: Option<UsageCategoryCost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fast_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ultrafast_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_premium_usd: Option<f64>,
    pub cost_source: String,
    pub records: usize,
    pub unpriced_records: usize,
    pub sessions: usize,
}
#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSourceFingerprint {
    pub host_id: String,
    pub provider: String,
    pub resolved_home_path: String,
    pub volume_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSourceStatus {
    Ok,
    Missing,
    Partial,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSource {
    pub fingerprint: UsageSourceFingerprint,
    pub status: UsageSourceStatus,
    pub scanned_files: usize,
    pub skipped_files: usize,
    pub malformed_records: usize,
    pub distinct_sessions: usize,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshing: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsagePricing {
    pub status: String,
    pub source: String,
    pub fetched_at: Option<String>,
    pub known_models: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageModelSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub model: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UsageModelFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<UsageModelSelector>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<UsageModelSelector>>,
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub contract_version: u32,
    pub read_at: String,
    pub time_zone: String,
    pub since_day: String,
    pub until_day: String,
    pub buckets: Vec<UsageBucket>,
    pub sources: Vec<UsageSource>,
    pub pricing: UsagePricing,
    pub scan_duration_ms: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_filter: Option<UsageModelFilter>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawUsageSummary {
    pub contract_version: u32,
    pub read_at: String,
    pub time_zone: String,
    pub since_day: String,
    pub until_day: String,
    pub buckets: Vec<UsageBucket>,
    pub sources: Vec<UsageSource>,
    pub pricing: UsagePricing,
    pub scan_duration_ms: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_filter: Option<UsageModelFilter>,
}

impl<'de> Deserialize<'de> for UsageSummary {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let mut value = serde_json::Value::deserialize(deserializer)?;
        normalize_summary(&mut value).map_err(D::Error::custom)?;
        let raw: RawUsageSummary = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(Self {
            contract_version: raw.contract_version,
            read_at: raw.read_at,
            time_zone: raw.time_zone,
            since_day: raw.since_day,
            until_day: raw.until_day,
            buckets: raw.buckets,
            sources: raw.sources,
            pricing: raw.pricing,
            scan_duration_ms: raw.scan_duration_ms,
            model_filter: raw.model_filter,
        })
    }
}

fn trim_field(v: &mut serde_json::Value, key: &str, optional: bool) -> bool {
    match v.get_mut(key) {
        None => optional,
        Some(serde_json::Value::String(s)) => {
            *s = s
                .trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                .to_owned();
            !s.is_empty()
        }
        _ => false,
    }
}
fn day(v: &mut serde_json::Value, key: &str) -> bool {
    trim_field(v, key, false)
        && v[key].as_str().is_some_and(|s| {
            let b = s.as_bytes();
            b.len() == 10
                && b[4] == b'-'
                && b[7] == b'-'
                && b.iter()
                    .enumerate()
                    .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        })
}
fn provider(v: &serde_json::Value, key: &str) -> bool {
    matches!(
        v.get(key).and_then(|x| x.as_str()),
        Some("claude" | "codex" | "grok" | "cursor" | "opencode" | "antigravity" | "pi" | "muse")
    )
}
fn nonnegative(v: &serde_json::Value, keys: &[&str]) -> bool {
    keys.iter().all(|k| {
        v.get(*k)
            .and_then(|x| x.as_f64())
            .is_some_and(|n| n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n < i64::MAX as f64)
    })
}
fn numbers(v: &serde_json::Value, required: &[&str], optional: &[&str]) -> bool {
    let number = |x: &serde_json::Value| x.as_f64().is_some_and(f64::is_finite);
    required.iter().all(|k| v.get(*k).is_some_and(number))
        && optional.iter().all(|k| v.get(*k).is_none_or(number))
}
fn normalize_ints(v: &mut serde_json::Value, keys: &[&str]) {
    for key in keys {
        if let Some(n) = v.get(*key).and_then(|x| x.as_f64()) {
            v[*key] = serde_json::Value::from(n as i64);
        }
    }
}
fn normalize_bucket(v: &mut serde_json::Value) -> bool {
    let totals = [
        "uncachedInputTokens",
        "cachedInputTokens",
        "cacheCreationTokens",
        "outputTokens",
        "reasoningTokens",
    ];
    let counts = ["records", "unpricedRecords", "sessions"];
    if !numbers(
        v,
        &["costUsd", "cacheSavingsUsd"],
        &["fastCostUsd", "ultrafastCostUsd", "speedPremiumUsd"],
    ) || v
        .get("categoryCostUsd")
        .is_some_and(|c| !numbers(c, &["input", "cacheRead", "cacheWrite", "output"], &[]))
        || !v.get("totals").is_some_and(|t| nonnegative(t, &totals))
        || !nonnegative(v, &counts)
    {
        return false;
    }
    normalize_ints(v.get_mut("totals").unwrap(), &totals);
    normalize_ints(v, &counts);
    day(v, "day")
        && trim_field(v, "hourStart", true)
        && provider(v, "provider")
        && trim_field(v, "model", false)
        && trim_field(v, "sourcePath", true)
        && v.get("totals").is_some_and(|t| {
            nonnegative(
                t,
                &[
                    "uncachedInputTokens",
                    "cachedInputTokens",
                    "cacheCreationTokens",
                    "outputTokens",
                    "reasoningTokens",
                ],
            )
        })
        && nonnegative(v, &["records", "unpricedRecords", "sessions"])
        && matches!(
            v.get("costSource").and_then(|x| x.as_str()),
            Some("providerReported" | "modelPriced" | "unpriced")
        )
        && serde_json::from_value::<UsageBucket>(v.clone()).is_ok()
}
fn normalize_source(v: &mut serde_json::Value) -> bool {
    let counts = [
        "scannedFiles",
        "skippedFiles",
        "malformedRecords",
        "distinctSessions",
    ];
    if !nonnegative(v, &counts) {
        return false;
    }
    normalize_ints(v, &counts);
    let valid = v.get_mut("fingerprint").is_some_and(|f| {
        trim_field(f, "hostId", false)
            && provider(f, "provider")
            && trim_field(f, "resolvedHomePath", false)
            && f.get("volumeId").is_some_and(|x| x.is_string())
    });
    valid
        && nonnegative(
            v,
            &[
                "scannedFiles",
                "skippedFiles",
                "malformedRecords",
                "distinctSessions",
            ],
        )
        && (v.get("message").is_some_and(|m| m.is_null()) || trim_field(v, "message", false))
        && v.get("refreshing").is_none_or(|x| x == true)
        && serde_json::from_value::<UsageSource>(v.clone()).is_ok()
}
fn normalize_filter(v: &mut serde_json::Value) -> bool {
    if !v.is_object() {
        return false;
    }
    for key in ["include", "exclude"] {
        if let Some(values) = v.get_mut(key) {
            let Some(values) = values.as_array_mut() else {
                return false;
            };
            for selector in values {
                if !trim_field(selector, "model", false) || !trim_field(selector, "provider", true)
                {
                    return false;
                }
            }
        }
    }
    true
}
fn normalize_summary(v: &mut serde_json::Value) -> Result<(), &'static str> {
    if !v.is_object()
        || !nonnegative(v, &["contractVersion", "scanDurationMs"])
        || !trim_field(v, "timeZone", false)
        || !day(v, "sinceDay")
        || !day(v, "untilDay")
        || !nonnegative(v, &["scanDurationMs"])
    {
        return Err("invalid usage summary");
    }
    normalize_ints(v, &["contractVersion", "scanDurationMs"]);
    if v.get_mut("modelFilter")
        .is_some_and(|f| !normalize_filter(f))
    {
        return Err("invalid model filter");
    }
    let Some(pricing) = v.get_mut("pricing") else {
        return Err("invalid pricing");
    };
    if !pricing
        .get("fetchedAt")
        .is_some_and(|x| x.is_null() || x.is_string())
    {
        return Err("invalid pricing");
    }
    if !matches!(
        pricing.get("status").and_then(|x| x.as_str()),
        Some("fresh" | "cached" | "unavailable")
    ) || !trim_field(pricing, "source", false)
        || !nonnegative(pricing, &["knownModels"])
    {
        return Err("invalid pricing");
    }
    normalize_ints(pricing, &["knownModels"]);
    for (key, normalize) in [
        (
            "buckets",
            normalize_bucket as fn(&mut serde_json::Value) -> bool,
        ),
        ("sources", normalize_source),
    ] {
        let Some(array) = v.get_mut(key).and_then(|a| a.as_array_mut()) else {
            return Err("invalid usage array");
        };
        array.retain_mut(normalize);
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentUsage {
    pub environment_id: EnvironmentId,
    pub label: String,
    pub summary: UsageSummary,
}
pub(crate) fn canonical_filter(
    filter: Option<UsageModelFilter>,
) -> Result<Option<UsageModelFilter>, &'static str> {
    let Some(filter) = filter else {
        return Ok(None);
    };
    let mut value = serde_json::to_value(filter).map_err(|_| "invalid model filter")?;
    if !normalize_filter(&mut value) {
        return Err("invalid model filter");
    }
    let mut filter: UsageModelFilter =
        serde_json::from_value(value).map_err(|_| "invalid model filter")?;
    let canonical = |items: &mut Vec<UsageModelSelector>| {
        let key = |s: &UsageModelSelector| {
            serde_json::to_string(&(s.provider.as_deref(), &s.model)).unwrap()
        };
        items.sort_by(|a, b| key(a).encode_utf16().cmp(key(b).encode_utf16()));
        items.dedup();
    };
    if let Some(items) = &mut filter.include {
        canonical(items);
    }
    if let Some(items) = &mut filter.exclude {
        canonical(items);
        if items.is_empty() {
            filter.exclude = None;
        }
    }
    if filter.include.is_none() && filter.exclude.is_none() {
        Ok(None)
    } else {
        Ok(Some(filter))
    }
}

/// The exact request associated with a response, including bounds the server does not echo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageQuery {
    pub since_day: String,
    pub until_day: String,
    pub time_zone: String,
    pub resolution: String,
    pub since_time: Option<String>,
    pub until_time: Option<String>,
    pub model_filter: Option<UsageModelFilter>,
}

/// A response bound to the request that produced it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageContribution {
    query: UsageQuery,
    environment: EnvironmentUsage,
    local_fallback: bool,
}
impl UsageContribution {
    pub fn query(&self) -> &UsageQuery {
        &self.query
    }
    pub fn environment_id(&self) -> &EnvironmentId {
        &self.environment.environment_id
    }
    pub(crate) fn mark_local_fallback(&mut self) {
        self.local_fallback = true;
    }
    /// Bind a decoded response to the request used to obtain it.
    pub fn bind(
        mut query: UsageQuery,
        environment: EnvironmentUsage,
    ) -> Result<Self, &'static str> {
        query.model_filter = canonical_filter(query.model_filter)?;
        let mut environment = environment;
        environment.summary.model_filter = canonical_filter(environment.summary.model_filter)?;
        let s = &environment.summary;
        if s.since_day != query.since_day
            || s.until_day != query.until_day
            || s.time_zone != query.time_zone
            || s.model_filter != query.model_filter
        {
            return Err("usage response does not match its query");
        }
        // Revalidate even when a caller constructed the public wire types directly.
        let summary = serde_json::to_value(s).map_err(|_| "invalid usage summary")?;
        let summary = serde_json::from_value(summary).map_err(|_| "invalid usage summary")?;
        Ok(Self {
            query,
            environment: EnvironmentUsage {
                summary,
                ..environment
            },
            local_fallback: false,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageContractMismatch {
    pub environment_id: EnvironmentId,
    pub direction: String,
    pub contract_version: u32,
}
#[derive(Clone, Debug)]
pub struct MergedUsage {
    /// Buckets retained after source ownership and partial scan reconciliation.
    pub buckets: Vec<UsageBucket>,
    pub sessions_by_provider: HashMap<String, usize>,
    pub duplicate_sources: Vec<String>,
    pub contributing_environments: Vec<EnvironmentId>,
    pub contract_mismatches: Vec<UsageContractMismatch>,
}
fn timestamp(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}
fn source_buckets<'a>(
    summary: &'a UsageSummary,
    source: &'a UsageSource,
) -> impl Iterator<Item = (usize, &'a UsageBucket)> + 'a {
    let count = summary
        .sources
        .iter()
        .filter(|s| s.fingerprint.provider == source.fingerprint.provider)
        .count();
    summary.buckets.iter().enumerate().filter(move |(_, b)| {
        b.provider == source.fingerprint.provider
            && (b.source_path.as_deref() == Some(&source.fingerprint.resolved_home_path)
                || (b.source_path.is_none() && count == 1))
    })
}
/// Merge only responses bound to the exact requested query. A live response replaces
/// a local fallback for the same environment, even if its read time is older.
pub fn merge_usage(
    contributions: &[UsageContribution],
    query: &UsageQuery,
    selection: &EnvironmentSelection,
    expected_version: u32,
) -> MergedUsage {
    let mut query = query.clone();
    let Ok(filter) = canonical_filter(query.model_filter.clone()) else {
        return merge_environments(&[], selection, expected_version);
    };
    query.model_filter = filter;
    let live: HashSet<_> = contributions
        .iter()
        .filter(|c| {
            !c.local_fallback
                && c.query == query
                && (USAGE_MERGE_COMPATIBLE_SINCE..=expected_version)
                    .contains(&c.environment.summary.contract_version)
        })
        .map(|c| &c.environment.environment_id)
        .collect();
    let environments: Vec<_> = contributions
        .iter()
        .filter(|c| {
            c.query == query && (!c.local_fallback || !live.contains(&c.environment.environment_id))
        })
        .map(|c| c.environment.clone())
        .collect();
    merge_environments(&environments, selection, expected_version)
}
fn merge_environments(
    environments: &[EnvironmentUsage],
    selection: &EnvironmentSelection,
    expected_version: u32,
) -> MergedUsage {
    let mut mismatches = Vec::new();
    let current: Vec<_> = environments
        .iter()
        .filter(|e| match selection {
            EnvironmentSelection::All => true,
            EnvironmentSelection::One(id) => &e.environment_id == id,
        })
        .filter(|e| {
            let v = e.summary.contract_version;
            if (USAGE_MERGE_COMPATIBLE_SINCE..=expected_version).contains(&v) {
                true
            } else {
                mismatches.push(UsageContractMismatch {
                    environment_id: e.environment_id.clone(),
                    direction: if v < expected_version {
                        "serverBehind"
                    } else {
                        "clientBehind"
                    }
                    .into(),
                    contract_version: v,
                });
                false
            }
        })
        .collect();
    // Node uses its default locale for localeCompare. Pin English for deterministic ownership.
    let collator =
        icu_collator::Collator::try_new(icu_locale::locale!("en").into(), Default::default())
            .expect("compiled English collation data");
    let mut ordered = current.clone();
    ordered.sort_by(|a, b| {
        timestamp(&b.summary.read_at)
            .unwrap_or(0)
            .cmp(&timestamp(&a.summary.read_at).unwrap_or(0))
            .then_with(|| collator.compare(a.environment_id.as_str(), b.environment_id.as_str()))
    });
    let mut owners: HashMap<&UsageSourceFingerprint, (&EnvironmentUsage, &UsageSource)> =
        HashMap::new();
    let mut sessions: HashMap<&UsageSourceFingerprint, usize> = HashMap::new();
    let mut duplicates = Vec::new();
    for status in [
        UsageSourceStatus::Ok,
        UsageSourceStatus::Partial,
        UsageSourceStatus::Failed,
    ] {
        for e in &ordered {
            for s in &e.summary.sources {
                if s.status != status {
                    continue;
                }
                if owners.contains_key(&s.fingerprint) {
                    duplicates.push(format!("{}: {}", e.label, s.fingerprint.resolved_home_path));
                } else {
                    owners.insert(&s.fingerprint, (e, s));
                    sessions.insert(&s.fingerprint, s.distinct_sessions);
                }
            }
        }
    }
    let mut seen: HashMap<
        &UsageSourceFingerprint,
        HashSet<(String, Option<String>, String, String)>,
    > = HashMap::new();
    let mut supplemental: HashMap<&EnvironmentId, HashSet<usize>> = HashMap::new();
    for e in &ordered {
        for s in &e.summary.sources {
            if s.status != UsageSourceStatus::Partial {
                continue;
            }
            let Some((owner, original)) = owners.get(&s.fingerprint) else {
                continue;
            };
            if original.status != UsageSourceStatus::Ok
                || timestamp(&e.summary.read_at)
                    .zip(timestamp(&owner.summary.read_at))
                    .map_or(true, |(a, b)| a <= b)
                || e.summary.model_filter != owner.summary.model_filter
            {
                continue;
            }
            let cells = seen.entry(&s.fingerprint).or_insert_with(|| {
                source_buckets(&owner.summary, original)
                    .map(|(_, b)| {
                        (
                            b.day.clone(),
                            b.hour_start.clone(),
                            b.provider.clone(),
                            b.model.clone(),
                        )
                    })
                    .collect()
            });
            let mut added = false;
            for (i, b) in source_buckets(&e.summary, s) {
                let key = (
                    b.day.clone(),
                    b.hour_start.clone(),
                    b.provider.clone(),
                    b.model.clone(),
                );
                if cells.insert(key) {
                    supplemental.entry(&e.environment_id).or_default().insert(i);
                    added = true;
                }
            }
            if added {
                let n = sessions.entry(&s.fingerprint).or_default();
                *n = (*n).max(s.distinct_sessions);
            }
        }
    }
    let mut result = MergedUsage {
        buckets: Vec::new(),
        sessions_by_provider: HashMap::new(),
        duplicate_sources: duplicates,
        contributing_environments: Vec::new(),
        contract_mismatches: mismatches,
    };
    for e in current {
        let mut providers = HashSet::new();
        let mut sources = HashSet::new();
        for s in &e.summary.sources {
            if s.status == UsageSourceStatus::Missing {
                continue;
            }
            if owners
                .get(&s.fingerprint)
                .is_some_and(|(owner, _)| owner.environment_id == e.environment_id)
            {
                providers.insert(s.fingerprint.provider.as_str());
                sources.insert((
                    s.fingerprint.provider.as_str(),
                    s.fingerprint.resolved_home_path.as_str(),
                ));
                *result
                    .sessions_by_provider
                    .entry(s.fingerprint.provider.clone())
                    .or_default() += sessions
                    .get(&s.fingerprint)
                    .copied()
                    .unwrap_or(s.distinct_sessions);
            }
        }
        let mut contributed = false;
        for (i, b) in e.summary.buckets.iter().enumerate() {
            if supplemental
                .get(&e.environment_id)
                .is_some_and(|v| v.contains(&i))
                || match &b.source_path {
                    Some(path) => sources.contains(&(b.provider.as_str(), path.as_str())),
                    None => providers.contains(b.provider.as_str()),
                }
            {
                result.buckets.push(b.clone());
                contributed = true;
            }
        }
        if contributed {
            result
                .contributing_environments
                .push(e.environment_id.clone());
        }
    }
    result
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCostQuality {
    pub provider_reported_share: f64,
    pub model_priced_share: f64,
    pub unpriced_share: f64,
    pub cache_savings_usd: f64,
}

impl MergedUsage {
    pub fn cost_quality(&self) -> UsageCostQuality {
        let records: usize = self.buckets.iter().map(|b| b.records).sum();
        let reported: usize = self
            .buckets
            .iter()
            .filter(|b| b.cost_source == "providerReported")
            .map(|b| b.records)
            .sum();
        let unpriced: usize = self.buckets.iter().map(|b| b.unpriced_records).sum();
        let ratio = |n: f64| {
            if records == 0 {
                0.0
            } else {
                n / records as f64
            }
        };
        UsageCostQuality {
            provider_reported_share: ratio(reported as f64),
            model_priced_share: ratio(records as f64 - reported as f64 - unpriced as f64),
            unpriced_share: ratio(unpriced as f64),
            cache_savings_usd: self.buckets.iter().map(|b| b.cache_savings_usd).sum(),
        }
    }
    /// Convert reconciled buckets into the existing presentation report for an explicit query window.
    pub fn to_report(&self, window: &crate::Window) -> crate::UsageReport {
        use crate::{
            Amount, Column, CostBySpeed, CostByType, Model, Period, Provider, Timeline, Tokens,
            UsageReport,
        };
        let hourly = window.period == Period::Day;
        let columns = if hourly {
            (0..24)
                .map(|i| {
                    let ms = window.since_ms + i as f64 * 3_600_000.0;
                    Column {
                        day: window.local_day(ms),
                        hour_start_ms: Some(ms),
                        by_provider: HashMap::new(),
                    }
                })
                .collect()
        } else {
            (window.since_day..=window.until_day)
                .map(|day| Column {
                    day,
                    hour_start_ms: None,
                    by_provider: HashMap::new(),
                })
                .collect()
        };
        let mut report = UsageReport {
            tokens: Tokens::default(),
            cost_usd: 0.0,
            cost_by_type: CostByType::default(),
            cost_by_speed: CostBySpeed::default(),
            cache_savings_usd: 0.0,
            providers: vec![],
            models: vec![],
            timeline: Timeline {
                resolution: if hourly { "hour" } else { "day" }.into(),
                timezone: window.timezone.to_string(),
                until_ms: if hourly { window.until_ms } else { 0.0 },
                columns,
            },
            sessions: self.sessions_by_provider.values().sum(),
            records: 0,
            unpriced_records: 0,
            duplicates_dropped: 0,
            rejected_files: 0,
            malformed_records: 0,
            cache_modified: None,
            scan_cache_missing: false,
            notices: vec![],
            local_sources: vec![],
        };
        let mut providers: HashMap<String, Provider> = HashMap::new();
        let mut models: HashMap<(String, String), Model> = HashMap::new();
        for (kind, count) in &self.sessions_by_provider {
            if *count == 0 {
                continue;
            }
            providers
                .entry(kind.clone())
                .or_insert_with(|| Provider {
                    kind: kind.clone(),
                    ..Default::default()
                })
                .sessions += count;
        }
        for b in &self.buckets {
            let t = Tokens {
                uncached_input: b.totals.uncached_input_tokens,
                cached_input: b.totals.cached_input_tokens,
                cache_creation: b.totals.cache_creation_tokens,
                output: b.totals.output_tokens,
                reasoning: b.totals.reasoning_tokens,
                processed: b.totals.total(),
            };
            fn add(a: &mut Tokens, b: &Tokens) {
                a.uncached_input += b.uncached_input;
                a.cached_input += b.cached_input;
                a.cache_creation += b.cache_creation;
                a.output += b.output;
                a.reasoning += b.reasoning;
                a.processed += b.processed;
            }
            add(&mut report.tokens, &t);
            report.cost_usd += b.cost_usd;
            report.cache_savings_usd += b.cache_savings_usd;
            report.records += b.records;
            report.unpriced_records += b.unpriced_records;
            if let Some(c) = &b.category_cost_usd {
                report.cost_by_type.input += c.input;
                report.cost_by_type.cache_read += c.cache_read;
                report.cost_by_type.cache_write += c.cache_write;
                report.cost_by_type.output += c.output;
            }
            report.cost_by_speed.fast += b.fast_cost_usd.unwrap_or(0.0);
            report.cost_by_speed.ultrafast += b.ultrafast_cost_usd.unwrap_or(0.0);
            report.cost_by_speed.premium += b.speed_premium_usd.unwrap_or(0.0);
            let p = providers
                .entry(b.provider.clone())
                .or_insert_with(|| Provider {
                    kind: b.provider.clone(),
                    ..Default::default()
                });
            add(&mut p.tokens, &t);
            p.cost_usd += b.cost_usd;
            p.records += b.records;
            let m = models
                .entry((b.provider.clone(), b.model.clone()))
                .or_insert_with(|| Model {
                    kind: b.provider.clone(),
                    name: b.model.clone(),
                    ..Default::default()
                });
            add(&mut m.tokens, &t);
            m.cost_usd += b.cost_usd;
            m.records += b.records;
            m.unpriced_records += b.unpriced_records;
            let index = if hourly {
                b.hour_start
                    .as_deref()
                    .and_then(timestamp)
                    .map(|ms| ((ms as f64 - window.since_ms) / 3_600_000.0).floor() as i64)
            } else {
                chrono::NaiveDate::parse_from_str(&b.day, "%Y-%m-%d")
                    .ok()
                    .map(|d| {
                        (d - chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()).num_days()
                            - window.since_day
                    })
            };
            if let Some(col) = index
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| report.timeline.columns.get_mut(i))
            {
                let amount: &mut Amount = col.by_provider.entry(b.provider.clone()).or_default();
                amount.cost_usd += b.cost_usd;
                amount.tokens += t.processed;
            }
        }
        let c = &mut report.cost_by_type;
        c.other = (report.cost_usd - c.input - c.cache_read - c.cache_write - c.output).max(0.0);
        report.cost_by_speed.standard =
            (report.cost_usd - report.cost_by_speed.fast - report.cost_by_speed.ultrafast).max(0.0);
        report.providers = providers.into_values().collect();
        report.providers.sort_by(|a, b| a.kind.cmp(&b.kind));
        report.models = models.into_values().collect();
        report.models.sort_by(|a, b| {
            b.cost_usd
                .total_cmp(&a.cost_usd)
                .then(b.tokens.processed.cmp(&a.tokens.processed))
                .then(a.kind.cmp(&b.kind))
                .then(a.name.cmp(&b.name))
        });
        report
    }
}
