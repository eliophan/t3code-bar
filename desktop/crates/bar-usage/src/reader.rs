use crate::{decode_cache, CacheError, Dataset, Period, PriceBook, UsageReport, Window};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use std::{
    fmt,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug)]
pub enum UsageError {
    Missing,
    Unreadable,
    UnsupportedVersion(Option<i64>),
}
impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "usage cache missing"),
            Self::Unreadable => write!(f, "usage cache unreadable"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported usage cache version: {v:?}"),
        }
    }
}
impl std::error::Error for UsageError {}
impl From<CacheError> for UsageError {
    fn from(e: CacheError) -> Self {
        match e {
            CacheError::Unreadable => Self::Unreadable,
            CacheError::UnsupportedVersion(v) => Self::UnsupportedVersion(v),
        }
    }
}
/// Transcript-cache reader. Configure local provider roots explicitly with `set_local_roots`.
pub struct UsageReader {
    state_dir: PathBuf,
    last_good: Option<UsageReport>,
    local_roots: crate::LocalRoots,
    local_cache: crate::local::LocalCache,
}
impl UsageReader {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            last_good: None,
            local_roots: crate::LocalRoots::default(),
            local_cache: crate::local::LocalCache::default(),
        }
    }
    pub fn set_local_roots(&mut self, roots: crate::LocalRoots) {
        self.local_roots = roots;
        self.local_cache = crate::local::LocalCache::default();
    }
    pub fn last_good(&self) -> Option<&UsageReport> {
        self.last_good.as_ref()
    }
    pub fn read(
        &mut self,
        period: Period,
        now: DateTime<Utc>,
        timezone: Tz,
    ) -> Result<UsageReport, UsageError> {
        let (dataset, local_sources, located) = self.read_dataset(now)?;
        let mut report = UsageReport::build(
            &dataset,
            &Window::new(period, now, timezone),
            &PriceBook::load(&self.state_dir),
        );
        report.local_sources = local_sources;
        report.cache_modified = located
            .as_ref()
            .map(|(_, modified)| DateTime::<Utc>::from(*modified));
        report.scan_cache_missing = located.is_none();
        self.last_good = Some(report.clone());
        Ok(report)
    }
    /// Read and normalize local records before presentation aggregation.
    pub fn read_contribution(
        &mut self,
        window: &Window,
        filter: Option<crate::UsageModelFilter>,
        provenance: &crate::LocalProvenance,
        environment_id: bar_identity::EnvironmentId,
        label: String,
        now: DateTime<Utc>,
    ) -> Result<crate::LocalContribution, UsageError> {
        let (dataset, _, _) = self.read_dataset(now)?;
        let prices = PriceBook::load(&self.state_dir);
        let fetched_at = std::fs::read(self.state_dir.join("usage-model-rates.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|doc| doc.get("fetchedAtMs").and_then(|v| v.as_i64()))
            .and_then(DateTime::<Utc>::from_timestamp_millis)
            .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        let known_models = prices.known_models();
        let pricing = crate::UsagePricing {
            status: if known_models == 0 {
                "unavailable"
            } else {
                "cached"
            }
            .into(),
            source: "local cache".into(),
            fetched_at,
            known_models,
        };
        crate::contribution::build_local(
            &dataset,
            &prices,
            pricing,
            window,
            filter,
            provenance,
            environment_id,
            label,
            now,
        )
        .map_err(|_| UsageError::Unreadable)
    }
    fn read_dataset(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<
        (
            Dataset,
            Vec<crate::LocalStatus>,
            Option<(PathBuf, SystemTime)>,
        ),
        UsageError,
    > {
        let located = match locate(&self.state_dir) {
            Ok(v) => Some(v),
            Err(UsageError::Missing) => None,
            Err(e) => return Err(e),
        };
        let mut dataset = if let Some((path, _)) = &located {
            let bytes = std::fs::read(path).map_err(|_| UsageError::Unreadable)?;
            let mut dataset: Dataset = decode_cache(&bytes)?;
            crate::resolve_codex_tiers(&mut dataset, &self.state_dir.join("statev2.sqlite"));
            dataset
        } else {
            Dataset {
                version: 11,
                sources: vec![],
                files: vec![],
                rejected_files: 0,
            }
        };
        let local_sources =
            crate::local::merge(&mut dataset, &self.local_roots, now, &mut self.local_cache);
        let mut token_budget = 0_i64;
        dataset.files.retain(|file| {
            let mut total = token_budget;
            for record in &file.records {
                for value in [
                    record.uncached_input,
                    record.cached_input,
                    record.cache_creation,
                    record.output,
                    record.reasoning,
                ] {
                    if value < 0 {
                        dataset.rejected_files += 1;
                        return false;
                    }
                    let Some(next) = total.checked_add(value) else {
                        dataset.rejected_files += 1;
                        return false;
                    };
                    total = next;
                }
            }
            token_budget = total;
            true
        });
        if located.is_none()
            && dataset.files.is_empty()
            && !local_sources
                .iter()
                .any(|s| s.state == "partial" || s.state == "unreadable")
        {
            return Err(UsageError::Missing);
        }
        Ok((dataset, local_sources, located))
    }
}
fn generation(name: &str) -> Option<u64> {
    let middle = name
        .strip_prefix("usage-scan-cache")?
        .strip_suffix(".json")?;
    if middle.is_empty() {
        return Some(0);
    }
    middle
        .strip_prefix("-v")?
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
}
fn locate(dir: &Path) -> Result<(PathBuf, SystemTime), UsageError> {
    let entries = std::fs::read_dir(dir).map_err(|_| UsageError::Missing)?;
    let mut candidates: Vec<_> = entries
        .flatten()
        .filter_map(|entry| {
            let n = generation(entry.file_name().to_str()?)?;
            let metadata = entry.metadata().ok()?;
            Some((n, entry.path(), metadata.modified().ok()?))
        })
        .collect();
    candidates.sort_by_key(|a| std::cmp::Reverse(a.0));
    if candidates.is_empty() {
        return Err(UsageError::Missing);
    }
    let mut skipped = Vec::new();
    for (_, path, modified) in candidates {
        let version = std::fs::read(&path)
            .ok()
            .and_then(|data| header_version(&data));
        let Some(version) = version else {
            continue;
        };
        if !(4..=11).contains(&version) {
            skipped.push((version, modified));
            continue;
        }
        if let Some((version, _)) = skipped.iter().find(|(_, m)| *m > modified) {
            return Err(UsageError::UnsupportedVersion(Some(*version)));
        }
        return Ok((path, modified));
    }
    Err(UsageError::UnsupportedVersion(skipped.first().map(|v| v.0)))
}

fn header_version(data: &[u8]) -> Option<i64> {
    use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor};
    use std::cell::Cell;
    struct Header<'a>(&'a Cell<Option<i64>>);
    impl<'de> DeserializeSeed<'de> for Header<'_> {
        type Value = ();
        fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
            d.deserialize_map(self)
        }
    }
    impl<'de> Visitor<'de> for Header<'_> {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a cache object")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
            while let Some(key) = map.next_key::<String>()? {
                if key == "version" {
                    let n = map.next_value::<serde_json::Value>()?.as_f64();
                    self.0.set(
                        n.filter(|n| {
                            n.fract() == 0.0 && *n >= i64::MIN as f64 && *n < i64::MAX as f64
                        })
                        .map(|n| n as i64),
                    );
                    return Err(serde::de::Error::custom("header found"));
                }
                map.next_value::<IgnoredAny>()?;
            }
            Ok(())
        }
    }
    let found = Cell::new(None);
    let _ = Header(&found).deserialize(&mut serde_json::Deserializer::from_slice(data));
    found.get()
}
