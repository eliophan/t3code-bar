use crate::{Dataset, Record};
use chrono::{DateTime, Offset, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const HOUR: f64 = 3_600_000.0;
const DAY: f64 = 86_400_000.0;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Period {
    Day,
    Week,
    Month,
    Quarter,
}
impl Period {
    fn days(self) -> i64 {
        match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
            Self::Quarter => 90,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub day: i64,
    pub hour: i64,
}
#[derive(Debug, Clone)]
pub struct Window {
    pub period: Period,
    pub timezone: Tz,
    pub now_ms: f64,
    pub since_ms: f64,
    pub until_ms: f64,
    pub since_day: i64,
    pub until_day: i64,
}
impl Window {
    pub fn new(period: Period, now: DateTime<Utc>, timezone: Tz) -> Self {
        let now_ms = now.timestamp_millis() as f64;
        let until_ms = (now_ms / 60_000.0).floor() * 60_000.0;
        let since_ms = until_ms - 24.0 * HOUR;
        let day = |ms| local_day(ms, timezone);
        let today = day(now_ms);
        let (since_day, until_day) = if period == Period::Day {
            (day(since_ms), day(until_ms))
        } else {
            (today - period.days() + 1, today)
        };
        Self {
            period,
            timezone,
            now_ms,
            since_ms,
            until_ms,
            since_day,
            until_day,
        }
    }
    pub fn local_day(&self, ms: f64) -> i64 {
        local_day(ms, self.timezone)
    }
    pub fn slot(&self, ms: f64) -> Option<Slot> {
        if !ms.is_finite() {
            return None;
        }
        let day = self.local_day(ms);
        if self.period == Period::Day {
            (ms >= self.since_ms && ms < self.until_ms).then(|| Slot {
                day,
                hour: ((ms - self.since_ms) / HOUR).floor() as i64,
            })
        } else {
            (day >= self.since_day && day <= self.until_day).then_some(Slot { day, hour: -1 })
        }
    }
    pub fn file_cutoff_ms(&self) -> f64 {
        let start = if self.period == Period::Day {
            self.since_ms
        } else {
            self.since_day as f64 * DAY
        };
        (start - 129_600_000.0).max(self.now_ms - 7_948_800_000.0)
    }
}
fn local_day(ms: f64, tz: Tz) -> i64 {
    let millis = ms.floor() as i64;
    let offset = DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&tz).offset().fix().local_minus_utc())
        .unwrap_or(0);
    ((ms + f64::from(offset) * 1000.0) / DAY).floor() as i64
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub kind: String,
    pub directory: String,
    pub distinct_sessions: usize,
    pub malformed: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub sources: Vec<Tally>,
    pub duplicates_dropped: usize,
    pub out_of_window: usize,
}
impl Selection {
    pub fn sessions(&self) -> usize {
        self.sources.iter().map(|s| s.distinct_sessions).sum()
    }
}
#[derive(Hash, PartialEq, Eq, Clone)]
struct SharedKey {
    session: String,
    timestamp: u64,
    model: String,
    tokens: [i64; 5],
}
#[derive(Hash, PartialEq, Eq)]
enum Key {
    Text(String),
    Shared(SharedKey, usize),
}
fn normalized(path: &str) -> Vec<&str> {
    let mut result = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => (),
            ".." => {
                result.pop();
            }
            _ => result.push(part),
        }
    }
    result
}
fn within(path: &str, directory: &str) -> bool {
    let irregular =
        |s: &str| s.contains("//") || s.split('/').any(|part| part == "." || part == "..");
    if irregular(path) || irregular(directory) {
        return normalized(path).starts_with(&normalized(directory));
    }
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| directory.ends_with('/') || rest.starts_with('/'))
}
impl Dataset {
    /// Visit surviving records in source and file order. Dedupe is applied before the window.
    pub fn select(
        &self,
        window: &Window,
        visit: impl FnMut(&str, usize, &Record, Slot),
    ) -> Selection {
        self.select_filtered(window, |_, _| true, visit)
    }
    pub(crate) fn select_filtered(
        &self,
        window: &Window,
        mut include: impl FnMut(&str, &Record) -> bool,
        mut visit: impl FnMut(&str, usize, &Record, Slot),
    ) -> Selection {
        let order = ["claude", "codex", "grok", "pi", "muse"];
        let ordered: Vec<_> = order
            .iter()
            .flat_map(|kind| self.sources.iter().filter(move |s| s.kind == *kind))
            .chain(
                self.sources
                    .iter()
                    .filter(|s| !order.contains(&s.kind.as_str())),
            )
            .collect();
        let groups: Vec<Vec<_>> = ordered
            .iter()
            .map(|source| {
                self.files
                    .iter()
                    .filter(|file| {
                        file.kind == source.kind
                            && file.modified_ms >= window.file_cutoff_ms()
                            && within(&file.path, &source.directory)
                    })
                    .collect()
            })
            .collect();
        let mut first_file = HashMap::new();
        let mut shared = HashSet::new();
        for (index, file) in groups.iter().flatten().enumerate() {
            if file.kind != "codex" {
                continue;
            }
            for r in &file.records {
                if !r.session.is_empty() {
                    match first_file.get(&r.session) {
                        Some(first) if *first != index => {
                            shared.insert(r.session.clone());
                        }
                        None => {
                            first_file.insert(r.session.clone(), index);
                        }
                        _ => (),
                    }
                }
            }
        }
        let mut seen = HashSet::new();
        let mut duplicates_dropped = 0;
        let mut out_of_window = 0;
        let mut tallies = Vec::new();
        for (source_index, group) in groups.iter().enumerate() {
            let mut sessions = HashSet::new();
            for file in group {
                let mut occurrences = HashMap::<SharedKey, usize>::new();
                for r in &file.records {
                    if !include(&file.kind, r) {
                        continue;
                    }
                    let key = if file.kind == "codex" && shared.contains(&r.session) {
                        let base = SharedKey {
                            session: r.session.clone(),
                            timestamp: r.timestamp_ms.to_bits(),
                            model: r.model.clone(),
                            tokens: [
                                r.uncached_input,
                                r.cached_input,
                                r.cache_creation,
                                r.output,
                                r.reasoning,
                            ],
                        };
                        let n = occurrences.entry(base.clone()).or_default();
                        *n += 1;
                        Some(Key::Shared(base, *n))
                    } else {
                        r.dedupe_key.as_ref().map(|k| Key::Text(k.clone()))
                    };
                    if let Some(key) = key {
                        if !seen.insert(key) {
                            duplicates_dropped += 1;
                            continue;
                        }
                    }
                    let Some(slot) = window.slot(r.timestamp_ms) else {
                        out_of_window += 1;
                        continue;
                    };
                    if !r.session.is_empty() {
                        sessions.insert(&r.session);
                    }
                    visit(&file.kind, source_index, r, slot);
                }
            }
            tallies.push(Tally {
                kind: ordered[source_index].kind.clone(),
                directory: ordered[source_index].directory.clone(),
                distinct_sessions: sessions.len(),
                malformed: group.iter().map(|f| f.malformed).sum(),
            });
        }
        Selection {
            sources: tallies,
            duplicates_dropped,
            out_of_window,
        }
    }
}
