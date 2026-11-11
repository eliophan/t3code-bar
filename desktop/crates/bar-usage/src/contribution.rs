use crate::{
    Dataset, EnvironmentUsage, Period, PriceBook, Speed, UsageBucket, UsageCategoryCost,
    UsageContribution, UsageModelFilter, UsagePricing, UsageQuery, UsageSource,
    UsageSourceFingerprint, UsageSourceStatus, UsageSummary, UsageTokenTotals, Window,
};
use bar_identity::EnvironmentId;
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// Verified identity supplied by the owner of a local T3 instance.
#[derive(Clone, Debug, Default)]
pub struct LocalProvenance {
    /// T3's base directory, which contains the read-only `usage-host-id` file.
    pub base_dir: PathBuf,
    /// The instance's configured T3CODE_HOST_ID, when known.
    pub host_id_override: Option<String>,
    /// Verified per-source overrides, including sources without cached volume metadata.
    pub source_overrides: Vec<UsageSourceFingerprint>,
}

/// Sources without verified identity are excluded instead of being assigned a fabricated identity.
pub struct LocalContribution {
    pub contribution: UsageContribution,
    pub unknown_sources: Vec<(String, String)>,
}

impl UsageQuery {
    pub fn for_window(window: &Window, model_filter: Option<UsageModelFilter>) -> Self {
        let iso = |ms: f64| {
            DateTime::<Utc>::from_timestamp_millis(ms as i64)
                .unwrap()
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        };
        let day = |days: i64| {
            (NaiveDate::from_ymd_opt(1970, 1, 1).unwrap() + chrono::Duration::days(days))
                .to_string()
        };
        let hourly = window.period == Period::Day;
        Self {
            since_day: day(window.since_day),
            until_day: day(window.until_day),
            time_zone: window.timezone.to_string(),
            resolution: if hourly { "hour" } else { "day" }.into(),
            since_time: hourly.then(|| iso(window.since_ms)),
            until_time: hourly.then(|| iso(window.until_ms)),
            model_filter,
        }
    }
}

fn matches_filter(filter: &Option<UsageModelFilter>, provider: &str, model: &str) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    let matches = |s: &crate::UsageModelSelector| {
        s.model == model.trim() && s.provider.as_deref().is_none_or(|p| p == provider)
    };
    filter
        .include
        .as_ref()
        .is_none_or(|items| items.iter().any(matches))
        && !filter
            .exclude
            .as_ref()
            .is_some_and(|items| items.iter().any(matches))
}

struct Cell {
    bucket: UsageBucket,
    sessions: HashSet<String>,
    reported: usize,
}

pub(crate) fn build_local(
    dataset: &Dataset,
    prices: &PriceBook,
    pricing: UsagePricing,
    window: &Window,
    filter: Option<UsageModelFilter>,
    provenance: &LocalProvenance,
    environment_id: EnvironmentId,
    label: String,
    now: DateTime<Utc>,
) -> Result<LocalContribution, &'static str> {
    let query = UsageQuery::for_window(window, crate::merge::canonical_filter(filter)?);
    let host = provenance
        .host_id_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            if provenance.base_dir.as_os_str().is_empty() {
                return None;
            }
            std::fs::read_to_string(provenance.base_dir.join("usage-host-id"))
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        });
    let mut fingerprints = HashMap::new();
    let mut unknown_sources = Vec::new();
    let mut verified = dataset.clone();
    verified.sources.retain(|source| {
        let key = (source.kind.clone(), source.directory.clone());
        let fingerprint = provenance
            .source_overrides
            .iter()
            .find(|p| {
                p.provider == source.kind
                    && p.resolved_home_path == source.directory
                    && !p.host_id.trim().is_empty()
                    && !p.resolved_home_path.trim().is_empty()
            })
            .cloned()
            .or_else(|| {
                Some(UsageSourceFingerprint {
                    host_id: host.clone()?,
                    provider: source.kind.clone(),
                    resolved_home_path: source.directory.clone(),
                    volume_id: source.volume_id.clone()?,
                })
            });
        if let Some(fingerprint) = fingerprint {
            fingerprints.insert(key, fingerprint);
            true
        } else {
            unknown_sources.push(key);
            false
        }
    });
    let mut cells: Vec<Cell> = Vec::new();
    let mut indexes = HashMap::new();
    let mut sessions: HashMap<usize, HashSet<String>> = HashMap::new();
    let selected = verified.select_filtered(
        window,
        |provider, record| matches_filter(&query.model_filter, provider, &record.model),
        |provider, source, input, slot| {
            let mut record = input.clone();
            if let Some(alias) = prices.alias(&record.model) {
                record.model = alias.into();
                record.rate_model = None;
            }
            if !record.session.is_empty() {
                sessions
                    .entry(source)
                    .or_default()
                    .insert(record.session.clone());
            }
            let key = (slot.day, slot.hour, source, record.model.clone());
            let index = *indexes.entry(key).or_insert_with(|| {
                let day = (NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()
                    + chrono::Duration::days(slot.day))
                .to_string();
                let hour_start = (window.period == Period::Day).then(|| {
                    DateTime::<Utc>::from_timestamp_millis(
                        (window.since_ms + slot.hour as f64 * 3_600_000.0) as i64,
                    )
                    .unwrap()
                    .to_rfc3339_opts(SecondsFormat::Millis, true)
                });
                cells.push(Cell {
                    bucket: UsageBucket {
                        day,
                        hour_start,
                        provider: provider.into(),
                        model: record.model.clone(),
                        source_path: None,
                        totals: UsageTokenTotals::default(),
                        cost_usd: 0.0,
                        cache_savings_usd: 0.0,
                        category_cost_usd: None,
                        fast_cost_usd: None,
                        ultrafast_cost_usd: None,
                        speed_premium_usd: None,
                        cost_source: "modelPriced".into(),
                        records: 0,
                        unpriced_records: 0,
                        sessions: 0,
                    },
                    sessions: HashSet::new(),
                    reported: 0,
                });
                cells.len() - 1
            });
            let cell = &mut cells[index];
            let b = &mut cell.bucket;
            b.totals.uncached_input_tokens += record.uncached_input;
            b.totals.cached_input_tokens += record.cached_input;
            b.totals.cache_creation_tokens += record.cache_creation;
            b.totals.output_tokens += record.output;
            b.totals.reasoning_tokens += record.reasoning;
            let price = prices.price(&record);
            b.cost_usd += price.cost_usd;
            b.cache_savings_usd += price.cache_savings_usd;
            if let Some(c) = price.categories {
                let total = b
                    .category_cost_usd
                    .get_or_insert_with(UsageCategoryCost::default);
                total.input += c.input;
                total.cache_read += c.cache_read;
                total.cache_write += c.cache_write;
                total.output += c.output;
            }
            if record.speed == Speed::Fast {
                *b.fast_cost_usd.get_or_insert(0.0) += price.cost_usd;
            }
            if record.speed == Speed::Ultrafast {
                *b.ultrafast_cost_usd.get_or_insert(0.0) += price.cost_usd;
            }
            *b.speed_premium_usd.get_or_insert(0.0) += price.speed_premium_usd;
            b.records += 1;
            b.unpriced_records += usize::from(price.unpriced);
            cell.reported += usize::from(price.provider_reported);
            if !record.session.is_empty() {
                cell.sessions.insert(record.session);
            }
        },
    );
    let mut sources = Vec::new();
    let mut known = HashMap::new();
    for (i, source) in selected.sources.iter().enumerate() {
        let fingerprint = fingerprints[&(source.kind.clone(), source.directory.clone())].clone();
        known.insert(i, fingerprint.resolved_home_path.clone());
        sources.push(UsageSource {
            fingerprint,
            status: UsageSourceStatus::Partial,
            scanned_files: 0,
            skipped_files: 0,
            malformed_records: source.malformed.max(0) as usize,
            distinct_sessions: sessions.get(&i).map_or(0, HashSet::len),
            message: None,
            refreshing: None,
        });
    }
    let mut buckets = Vec::new();
    for ((_, _, source, _), index) in indexes {
        let Some(path) = known.get(&source) else {
            continue;
        };
        let cell = &mut cells[index];
        let b = &mut cell.bucket;
        b.source_path = Some(path.clone());
        b.sessions = cell.sessions.len();
        b.cost_source = if b.unpriced_records == b.records {
            "unpriced"
        } else if cell.reported == b.records {
            "providerReported"
        } else {
            "modelPriced"
        }
        .into();
        let round = |v: f64| (v * 1e6 + 0.5).floor() / 1e6;
        if let Some(c) = &mut b.category_cost_usd {
            c.input = round(c.input);
            c.cache_read = round(c.cache_read);
            c.cache_write = round(c.cache_write);
            c.output = round(c.output);
        }
        for value in [
            &mut b.fast_cost_usd,
            &mut b.ultrafast_cost_usd,
            &mut b.speed_premium_usd,
        ] {
            *value = value.map(round).filter(|v| *v != 0.0);
        }
        buckets.push(b.clone());
    }
    buckets.sort_by(|a, b| {
        (&a.day, &a.hour_start, &a.provider, &a.model, &a.source_path).cmp(&(
            &b.day,
            &b.hour_start,
            &b.provider,
            &b.model,
            &b.source_path,
        ))
    });
    let summary = UsageSummary {
        contract_version: crate::USAGE_CONTRACT_VERSION,
        read_at: now.to_rfc3339_opts(SecondsFormat::Millis, true),
        time_zone: query.time_zone.clone(),
        since_day: query.since_day.clone(),
        until_day: query.until_day.clone(),
        buckets,
        sources,
        pricing,
        scan_duration_ms: 0,
        model_filter: query.model_filter.clone(),
    };
    let mut contribution = UsageContribution::bind(
        query,
        EnvironmentUsage {
            environment_id,
            label,
            summary,
        },
    )?;
    contribution.mark_local_fallback();
    Ok(LocalContribution {
        contribution,
        unknown_sources,
    })
}
