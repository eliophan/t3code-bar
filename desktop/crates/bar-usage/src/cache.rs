use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheError {
    Unreadable,
    UnsupportedVersion(Option<i64>),
}
impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable => write!(f, "unreadable usage cache"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported usage cache version: {v:?}"),
        }
    }
}
impl Error for CacheError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub kind: String,
    pub directory: String,
    #[serde(default)]
    pub volume_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    pub path: String,
    pub kind: String,
    pub modified_ms: f64,
    pub malformed: i64,
    pub records: Vec<Record>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Speed {
    Standard,
    Fast,
    Ultrafast,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub timestamp_ms: f64,
    pub model: String,
    pub rate_model: Option<String>,
    pub session: String,
    pub uncached_input: i64,
    pub cached_input: i64,
    pub cache_creation: i64,
    pub output: i64,
    pub reasoning: i64,
    pub reported_cost_usd: Option<f64>,
    pub speed: Speed,
    pub dedupe_key: Option<String>,
    pub unresolved_tier_turn: Option<String>,
}
impl Record {
    pub fn processed(&self) -> i64 {
        self.uncached_input + self.cached_input + self.cache_creation + self.output
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dataset {
    pub version: i64,
    pub sources: Vec<Source>,
    pub files: Vec<File>,
    pub rejected_files: usize,
}

fn index(v: &Value, len: usize) -> Option<usize> {
    let n = v.as_f64()?;
    if n.fract() != 0.0 || n < 0.0 || n >= len as f64 {
        None
    } else {
        Some(n as usize)
    }
}
fn token(v: &Value) -> Option<i64> {
    let n = v.as_f64()?;
    if n.abs() >= 9e15 {
        None
    } else {
        Some(n as i64)
    }
}
fn row(v: &Value, models: &[String], sessions: &[String], version: i64) -> Option<Record> {
    let a = v.as_array()?;
    if a.len() < 11 {
        return None;
    }
    let model = models.get(index(&a[1], models.len())?)?.clone();
    let speed = match index(&a[10], 3)? {
        0 => Speed::Standard,
        1 => Speed::Fast,
        _ => Speed::Ultrafast,
    };
    let rate_model = if let Some(v) = a.get(11) {
        if v.is_null() {
            None
        } else {
            Some(models.get(index(v, models.len())?)?.clone())
        }
    } else {
        None
    };
    Some(Record {
        timestamp_ms: a[0].as_f64()?,
        model,
        rate_model,
        session: index(&a[2], sessions.len())
            .and_then(|i| sessions.get(i))
            .cloned()
            .unwrap_or_default(),
        uncached_input: token(&a[3])?,
        cached_input: token(&a[4])?,
        cache_creation: token(&a[5])?,
        output: token(&a[6])?,
        reasoning: token(&a[7])?,
        dedupe_key: a[8].as_str().map(str::to_owned),
        reported_cost_usd: a[9].as_f64(),
        speed,
        unresolved_tier_turn: if version >= 7 {
            a.get(12).and_then(Value::as_str).map(str::to_owned)
        } else {
            None
        },
    })
}
fn strings(v: Option<&Value>) -> Result<Vec<String>, CacheError> {
    match v {
        None => Ok(vec![]),
        Some(v) => v
            .as_array()
            .ok_or(CacheError::Unreadable)?
            .iter()
            .map(|x| {
                if x.is_null() {
                    Ok(String::new())
                } else {
                    x.as_str().map(str::to_owned).ok_or(CacheError::Unreadable)
                }
            })
            .collect(),
    }
}
/// Decode a complete scan cache. Object insertion order is preserved for first-wins deduplication.
pub fn decode_cache(bytes: &[u8]) -> Result<Dataset, CacheError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|_| CacheError::Unreadable)?;
    let obj = root.as_object().ok_or(CacheError::Unreadable)?;
    let version = obj
        .get("version")
        .and_then(Value::as_f64)
        .filter(|n| n.fract() == 0.0 && *n >= i64::MIN as f64 && *n < i64::MAX as f64)
        .map(|n| n as i64);
    let version = match version {
        Some(v @ 4..=11) => v,
        other => return Err(CacheError::UnsupportedVersion(other)),
    };
    let models = strings(obj.get("models"))?;
    let sessions = strings(obj.get("sessions"))?;
    let mut sources = Vec::new();
    if let Some(v) = obj.get("sources") {
        for (key, source) in v.as_object().ok_or(CacheError::Unreadable)? {
            let source = source.as_object().ok_or(CacheError::Unreadable)?;
            if let Some(dir) = source.get("dir").and_then(Value::as_str) {
                sources.push(Source {
                    kind: key.split('\0').next().unwrap_or(key).into(),
                    directory: dir.into(),
                    volume_id: source
                        .get("volumeId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
        }
    }
    let mut files = Vec::new();
    let mut rejected_files = 0;
    let mut budget = 0_i64;
    for (path, value) in obj
        .get("files")
        .and_then(Value::as_object)
        .ok_or(CacheError::Unreadable)?
    {
        let object = value.as_object().ok_or(CacheError::Unreadable)?;
        let mut provider_seen = false;
        for (key, value) in object {
            if key == "p" && value.is_string() {
                provider_seen = true;
            }
            if (key == "r" || key == "t") && provider_seen {
                let rows = value.as_array().ok_or(CacheError::Unreadable)?;
                if rows.iter().any(|row| !row.is_array()) {
                    return Err(CacheError::Unreadable);
                }
            }
        }
        if object.get("p").is_some_and(Value::is_string)
            && object.get("m").is_some_and(Value::is_number)
        {
            for (key, value) in object {
                if key == "p" {
                    break;
                }
                if key == "r" || key == "t" {
                    let rows = value.as_array().ok_or(CacheError::Unreadable)?;
                    if rows.iter().any(|row| !row.is_array()) {
                        return Err(CacheError::Unreadable);
                    }
                }
            }
        }
        let parsed = (|| -> Option<(File, i64)> {
            let o = value.as_object()?;
            let kind = o.get("p")?.as_str()?;
            let modified_ms = o.get("m")?.as_f64()?;
            let malformed = o
                .get("mr")
                .and_then(|v| {
                    v.as_i64().or_else(|| {
                        v.as_f64()
                            .filter(|n| {
                                n.fract() == 0.0 && *n >= i64::MIN as f64 && *n < i64::MAX as f64
                            })
                            .map(|n| n as i64)
                    })
                })
                .unwrap_or(0);
            let mut records = Vec::new();
            let mut before_provider = Vec::new();
            let mut after_provider = Vec::new();
            let mut seen_provider = false;
            for (key, value) in o {
                if key == "p" {
                    seen_provider = true;
                }
                if key == "r" || key == "t" {
                    if seen_provider {
                        after_provider.push(value);
                    } else {
                        before_provider.push(value);
                    }
                }
            }
            for value in after_provider.into_iter().chain(before_provider) {
                for item in value.as_array()? {
                    let r = row(item, &models, &sessions, version)?;
                    if kind != "claude" || r.model != "<synthetic>" {
                        records.push(r);
                    }
                }
            }
            let mut total = budget;
            for r in &records {
                for n in [
                    r.uncached_input,
                    r.cached_input,
                    r.cache_creation,
                    r.output,
                    r.reasoning,
                ] {
                    if n < 0 {
                        return None;
                    }
                    total = total.checked_add(n)?;
                }
            }
            Some((
                File {
                    path: path.clone(),
                    kind: kind.into(),
                    modified_ms,
                    malformed,
                    records,
                },
                total,
            ))
        })();
        if let Some((file, total)) = parsed {
            files.push(file);
            budget = total;
        } else {
            rejected_files += 1;
        }
    }
    Ok(Dataset {
        version,
        sources,
        files,
        rejected_files,
    })
}
