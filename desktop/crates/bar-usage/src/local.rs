use crate::{Dataset, File, Record, Source, Speed};
use rusqlite::{types::ValueRef, Connection, OpenFlags, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

#[derive(Debug, Clone, Default)]
pub struct LocalRoots {
    pub cursor: Option<PathBuf>,
    pub opencode: Vec<PathBuf>,
    pub antigravity: Vec<PathBuf>,
}

impl LocalRoots {
    pub fn discover(state_dir: impl AsRef<Path>, home: impl AsRef<Path>) -> Self {
        let environment = std::env::vars().collect::<HashMap<_, _>>();
        Self::discover_with_environment(state_dir, home, &environment)
    }

    pub fn discover_with_environment(
        state_dir: impl AsRef<Path>,
        home: impl AsRef<Path>,
        environment: &HashMap<String, String>,
    ) -> Self {
        let state_dir = state_dir.as_ref();
        let home = home.as_ref();
        let expand = |value: &str| {
            let value = value.trim();
            if value == "~" {
                home.to_path_buf()
            } else if let Some(suffix) = value.strip_prefix("~/") {
                home.join(suffix)
            } else {
                PathBuf::from(value)
            }
        };
        let roots = |key: &str, defaults: Vec<PathBuf>| {
            environment
                .get(key)
                .filter(|value| !value.trim().is_empty())
                .map(|value| {
                    value
                        .split(',')
                        .filter(|part| !part.is_empty())
                        .map(expand)
                        .collect()
                })
                .unwrap_or(defaults)
        };
        let xdg = environment
            .get("XDG_DATA_HOME")
            .filter(|value| value.starts_with('/'))
            .map(PathBuf::from);
        let opencode_defaults = vec![xdg
            .unwrap_or_else(|| home.join(".local/share"))
            .join("opencode")];
        let antigravity_defaults = [
            "antigravity",
            "antigravity-cli",
            "antigravity-ide",
            "antigravity-backup",
        ]
        .into_iter()
        .map(|name| home.join(".gemini").join(name))
        .chain([home.join(".config/antigravity")])
        .collect();
        let mut antigravity = roots("ANTIGRAVITY_DATA_DIR", antigravity_defaults);
        for id in antigravity_instances(&state_dir.join("settings.json")) {
            let hash = format!("{:x}", Sha256::digest(id.as_bytes()));
            antigravity.push(
                state_dir
                    .join("providers/antigravity")
                    .join(hash)
                    .join("antigravity-acp"),
            );
        }
        Self {
            cursor: Some(state_dir.join("usage-cursor-account-cache-v1.json")),
            opencode: roots("OPENCODE_DATA_DIR", opencode_defaults),
            antigravity,
        }
    }
}

fn antigravity_instances(path: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(path) else {
        return vec![];
    };
    let Ok(Value::Object(root)) = serde_json::from_slice::<Value>(&bytes) else {
        return vec![];
    };
    let Some(Value::Object(instances)) = root.get("providerInstances") else {
        return vec![];
    };
    instances
        .iter()
        .filter_map(|(id, value)| {
            let Value::Object(instance) = value else {
                return None;
            };
            (instance.get("driver").and_then(Value::as_str) == Some("antigravity"))
                .then(|| id.clone())
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    pub kind: String,
    pub state: String,
    pub checked_at: chrono::DateTime<chrono::Utc>,
    pub data_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub failed_files: usize,
}

fn n(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite())
}

fn token(v: &Value) -> Option<i64> {
    n(v).filter(|n| *n >= 0.0 && *n < 9e15).map(|n| n as i64)
}

fn index(v: &Value, len: usize) -> Option<usize> {
    n(v).filter(|n| *n >= 0.0 && n.fract() == 0.0 && *n < len as f64)
        .map(|n| n as usize)
}

fn time(ms: f64) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::from_timestamp_millis(ms as i64)
}

fn record(timestamp_ms: f64, model: String, session: String) -> Record {
    Record {
        timestamp_ms,
        model,
        session,
        rate_model: None,
        uncached_input: 0,
        cached_input: 0,
        cache_creation: 0,
        output: 0,
        reasoning: 0,
        reported_cost_usd: None,
        speed: Speed::Standard,
        dedupe_key: None,
        unresolved_tier_turn: None,
    }
}

#[derive(Debug, Clone, Default)]
struct LocalUsageRead {
    records: Vec<Record>,
    candidates: Vec<crate::antigravity::Candidate>,
    accounts: Vec<CursorAccount>,
    malformed: i64,
    updated: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
struct CursorAccount {
    id: String,
    updated: chrono::DateTime<chrono::Utc>,
    records: Vec<Record>,
}

fn cursor(path: &Path) -> Result<LocalUsageRead, ()> {
    let bytes = std::fs::read(path).map_err(|_| ())?;
    let doc: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if doc.get("version").and_then(Value::as_i64) != Some(1) {
        return Err(());
    }
    let strings = doc
        .get("strings")
        .and_then(Value::as_array)
        .ok_or(())?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or(()))
        .collect::<Result<Vec<_>, _>>()?;
    let accounts = doc.get("accounts").and_then(Value::as_object).ok_or(())?;
    let mut keys: Vec<_> = accounts.keys().collect();
    keys.sort();
    let mut result = LocalUsageRead::default();
    for key in keys {
        let account = &accounts[key];
        let Some((rows, fetched)) = account.get("records").and_then(Value::as_array).zip(
            account
                .get("fetchedAtMs")
                .and_then(n)
                .filter(|n| *n > 0.0 && *n < 9e15),
        ) else {
            result.malformed += 1;
            continue;
        };
        if rows.iter().any(|row| !row.is_array()) {
            result.malformed += 1;
            continue;
        }
        let Some(date) = time(fetched) else {
            result.malformed += 1;
            continue;
        };
        result.updated = Some(result.updated.map_or(date, |old| old.min(date)));
        let mut records = Vec::new();
        for row in rows {
            let parsed = (|| {
                let row = row.as_array()?;
                if row.len() < 9 {
                    return None;
                }
                let ts = n(&row[0]).filter(|n| *n > 0.0 && *n < 9e15)?;
                let model = strings.get(index(&row[1], strings.len())?)?.clone();
                let session = strings.get(index(&row[2], strings.len())?)?.clone();
                let mut record = record(ts, model.clone(), session);
                record.rate_model = Some(model.strip_suffix("-thinking").unwrap_or(&model).into());
                record.uncached_input = token(&row[3])?;
                record.cached_input = token(&row[4])?;
                record.cache_creation = token(&row[5])?;
                record.output = token(&row[6])?;
                record.reported_cost_usd = Some(n(&row[7]).filter(|value| *value >= 0.0)?);
                record.dedupe_key = Some(row[8].as_str()?.into());
                Some(record)
            })();
            if let Some(record) = parsed {
                records.push(record);
            } else {
                result.malformed += 1;
            }
        }
        result.accounts.push(CursorAccount {
            id: key.clone(),
            updated: date,
            records,
        });
    }
    Ok(result)
}

#[derive(Debug, Deserialize)]
struct OpenCodeMessage {
    role: Option<String>,
    id: Option<String>,
    #[serde(rename = "sessionID")]
    session_id: Option<String>,
    #[serde(rename = "modelID")]
    model_id: Option<String>,
    model: Option<OpenCodeModel>,
    time: Option<OpenCodeTime>,
    tokens: Option<OpenCodeTokens>,
    cost: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeModel {
    id: Option<String>,
    #[serde(rename = "modelID")]
    model_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeTime {
    created: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeTokens {
    input: Option<f64>,
    output: Option<f64>,
    reasoning: Option<f64>,
    cache: Option<OpenCodeCache>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeCache {
    read: Option<f64>,
    write: Option<f64>,
}

fn opencode_message(
    value: &Value,
    id: &str,
    session: &str,
    timestamp: Option<f64>,
) -> Result<Option<Record>, ()> {
    let object = value.as_object().ok_or(())?;
    for key in ["model", "time", "tokens"] {
        if object
            .get(key)
            .is_some_and(|value| !value.is_null() && !value.is_object())
        {
            return Err(());
        }
    }
    if object
        .get("tokens")
        .and_then(Value::as_object)
        .and_then(|tokens| tokens.get("cache"))
        .is_some_and(|value| !value.is_null() && !value.is_object())
    {
        return Err(());
    }
    let message: OpenCodeMessage = serde_json::from_value(value.clone()).map_err(|_| ())?;
    if let Some(role) = message.role.as_deref() {
        if role != "assistant" {
            return Ok(None);
        }
    }
    let model = [
        message.model.as_ref().and_then(|model| model.id.as_deref()),
        message
            .model
            .as_ref()
            .and_then(|model| model.model_id.as_deref()),
        message.model_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|model| !model.is_empty());
    let timestamp = message
        .time
        .as_ref()
        .and_then(|time| time.created)
        .or(timestamp);
    let (Some(model), Some(timestamp)) = (
        model,
        timestamp.filter(|value| value.is_finite() && *value > 0.0),
    ) else {
        return Ok(None);
    };
    let session = if session.is_empty() {
        message.session_id.as_deref().unwrap_or("")
    } else {
        session
    };
    let mut record = record(timestamp, model.into(), session.into());
    let token_value = |value: Option<f64>| match value {
        None => Ok(0),
        Some(value) => token(&Value::from(value)).ok_or(()),
    };
    let tokens = message.tokens.as_ref();
    record.uncached_input = token_value(tokens.and_then(|tokens| tokens.input))?;
    record.cached_input = token_value(
        tokens
            .and_then(|tokens| tokens.cache.as_ref())
            .and_then(|cache| cache.read),
    )?;
    record.cache_creation = token_value(
        tokens
            .and_then(|tokens| tokens.cache.as_ref())
            .and_then(|cache| cache.write),
    )?;
    record.reasoning = token_value(tokens.and_then(|tokens| tokens.reasoning))?;
    record.output = token_value(tokens.and_then(|tokens| tokens.output))?
        .checked_add(record.reasoning)
        .ok_or(())?;
    record.reported_cost_usd = message
        .cost
        .filter(|value| value.is_finite() && *value > 0.0);
    let key = if id.is_empty() {
        message.id.as_deref().unwrap_or("")
    } else {
        id
    };
    if !key.is_empty() {
        record.dedupe_key = Some(format!("opencode:{key}"));
    }
    Ok((record.processed() > 0).then_some(record))
}

fn sqlite_text(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<String>> {
    Ok(match row.get_ref(index)? {
        ValueRef::Text(bytes) => Some(
            String::from_utf8_lossy(bytes.split(|byte| *byte == 0).next().unwrap_or_default())
                .into_owned(),
        ),
        _ => None,
    })
}

fn sqlite_double(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<f64>> {
    Ok(match row.get_ref(index)? {
        ValueRef::Null => None,
        ValueRef::Integer(value) => Some(value as f64),
        ValueRef::Real(value) => Some(value),
        ValueRef::Text(value) => Some(
            std::str::from_utf8(value)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0),
        ),
        ValueRef::Blob(_) => Some(0.0),
    })
}

fn walk(root: &Path, extension: &str, result: &mut Vec<PathBuf>) -> Result<(), ()> {
    let mut entries = std::fs::read_dir(root)
        .map_err(|_| ())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|_| ())?;
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, extension, result)?;
        } else if path.extension().is_some_and(|value| value == extension) {
            result.push(path);
        }
    }
    Ok(())
}

fn read_opencode(path: &Path) -> Result<LocalUsageRead, ()> {
    if path.extension().is_some_and(|value| value == "json") {
        let data = std::fs::read(path).map_err(|_| ())?;
        let value = serde_json::from_slice(&data).map_err(|_| ())?;
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let record = opencode_message(&value, id, "", None)?;
        let updated = std::fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .map(chrono::DateTime::<chrono::Utc>::from);
        return Ok(LocalUsageRead {
            records: record.into_iter().collect(),
            updated,
            ..LocalUsageRead::default()
        });
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )
    .map_err(|_| ())?;
    db.busy_timeout(Duration::from_millis(250))
        .map_err(|_| ())?;
    db.execute_batch("BEGIN DEFERRED").map_err(|_| ())?;
    let mut result = LocalUsageRead::default();
    let mut found = false;
    for table in ["message", "session_message"] {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
                [table],
                |row| row.get(0),
            )
            .map_err(|_| ())?;
        if !exists {
            continue;
        }
        found = true;
        let mut statement = db
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|_| ())?;
        let columns = statement
            .query_map([], |row| sqlite_text(row, 1))
            .map_err(|_| ())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ())?;
        let sql = format!(
            "SELECT id, session_id, data, {} FROM {}{} ORDER BY rowid",
            if columns
                .iter()
                .flatten()
                .any(|value| value == "time_created")
            {
                "CAST(time_created AS REAL)"
            } else {
                "NULL"
            },
            table,
            if table == "session_message" {
                " WHERE type = 'assistant'"
            } else {
                ""
            }
        );
        let mut statement = db.prepare(&sql).map_err(|_| ())?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    sqlite_text(row, 0)?,
                    sqlite_text(row, 1)?,
                    sqlite_text(row, 2)?,
                    sqlite_double(row, 3)?,
                ))
            })
            .map_err(|_| ())?;
        for row in rows {
            let (id, session, data, timestamp) = row.map_err(|_| ())?;
            let parsed = data
                .as_deref()
                .ok_or(())
                .and_then(|data| serde_json::from_str(data).map_err(|_| ()))
                .and_then(|value| {
                    opencode_message(
                        &value,
                        id.as_deref().unwrap_or(""),
                        session.as_deref().unwrap_or(""),
                        timestamp,
                    )
                });
            match parsed {
                Ok(Some(record)) => result.records.push(record),
                Ok(None) => (),
                Err(()) => result.malformed += 1,
            }
        }
    }
    if !found {
        return Err(());
    }
    Ok(result)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Stamp {
    fn capture(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            size: metadata.len(),
            modified,
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    file: Option<Stamp>,
    wal: Option<Stamp>,
}

impl Fingerprint {
    fn capture(path: &Path) -> Self {
        Self {
            file: Stamp::capture(path),
            wal: Stamp::capture(&{
                let mut name = path.as_os_str().to_os_string();
                name.push("-wal");
                PathBuf::from(name)
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedFile {
    fingerprint: Fingerprint,
    read: LocalUsageRead,
}

#[derive(Debug, Default)]
pub(crate) struct LocalCache {
    files: HashMap<String, CachedFile>,
}

impl LocalCache {
    fn read(
        &mut self,
        path: &Path,
        load: impl FnOnce() -> Result<LocalUsageRead, ()>,
    ) -> (Option<LocalUsageRead>, bool) {
        let key = path.to_string_lossy().into_owned();
        let fingerprint = Fingerprint::capture(path);
        let previous = self.files.get(&key).cloned();
        if previous
            .as_ref()
            .is_some_and(|cached| cached.fingerprint == fingerprint)
        {
            return (previous.map(|cached| cached.read), false);
        }
        match load() {
            Ok(fresh) if Fingerprint::capture(path) == fingerprint => {
                self.files.insert(
                    key,
                    CachedFile {
                        fingerprint,
                        read: fresh.clone(),
                    },
                );
                (Some(fresh), false)
            }
            _ => (previous.map(|cached| cached.read), true),
        }
    }
}

#[derive(Debug, Clone)]
struct LocalUsageFile {
    url: PathBuf,
    root: PathBuf,
    kind: &'static str,
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn discover(root: &Path, kind: &'static str) -> Result<Vec<LocalUsageFile>, ()> {
    if !root.exists() {
        return Ok(vec![]);
    }
    if kind == "antigravity" {
        let mut paths = Vec::new();
        walk(root, "db", &mut paths)?;
        return Ok(paths
            .into_iter()
            .map(|path| LocalUsageFile {
                url: canonical(&path),
                root: canonical(root),
                kind,
            })
            .collect());
    }
    let mut result = Vec::new();
    let mut entries = std::fs::read_dir(root)
        .map_err(|_| ())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())?;
    entries.sort_by_key(|entry| {
        if entry.file_name() == "opencode.db" {
            String::new()
        } else {
            entry.file_name().to_string_lossy().into_owned()
        }
    });
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().map_err(|_| ())?.is_symlink() {
            continue;
        }
        if name == "opencode.db"
            || (name.starts_with("opencode-")
                && name.ends_with(".db")
                && name.len() > 12
                && name[9..name.len() - 3]
                    .chars()
                    .all(|value| value.is_ascii_alphanumeric() || value == '_' || value == '-'))
        {
            result.push(LocalUsageFile {
                url: canonical(&entry.path()),
                root: canonical(root),
                kind,
            });
        }
    }
    let legacy = root.join("storage/message");
    if legacy.exists() {
        let mut paths = Vec::new();
        walk(&legacy, "json", &mut paths)?;
        result.extend(paths.into_iter().map(|path| LocalUsageFile {
            url: canonical(&path),
            root: canonical(root),
            kind,
        }));
    }
    Ok(result)
}

fn read_file(file: &LocalUsageFile) -> Result<LocalUsageRead, ()> {
    match file.kind {
        "opencode" => read_opencode(&file.url),
        "antigravity" => {
            let (candidates, malformed) = crate::antigravity::read(&file.url)?;
            Ok(LocalUsageRead {
                candidates,
                malformed,
                ..LocalUsageRead::default()
            })
        }
        "cursor" => cursor(&file.url),
        _ => Err(()),
    }
}

fn status(
    kind: &'static str,
    now: chrono::DateTime<chrono::Utc>,
    files: usize,
    failures: usize,
    malformed: i64,
    updated: Option<chrono::DateTime<chrono::Utc>>,
    provider_files: usize,
) -> LocalStatus {
    let state = if failures > 0 {
        if provider_files == 0 {
            "unreadable"
        } else {
            "partial"
        }
    } else if malformed > 0 {
        "partial"
    } else if files == 0 {
        "missing"
    } else {
        "ready"
    };
    LocalStatus {
        kind: kind.into(),
        state: state.into(),
        checked_at: now,
        data_updated_at: updated,
        failed_files: failures,
    }
}

fn scan_provider(
    dataset: &mut Dataset,
    kind: &'static str,
    roots: Vec<PathBuf>,
    now: chrono::DateTime<chrono::Utc>,
    cache: &mut LocalCache,
    retained: &mut HashSet<String>,
) -> LocalStatus {
    let mut discovered = Vec::new();
    let mut failures = 0;
    let mut seen = HashSet::new();
    for original in roots {
        let root = canonical(&original);
        let paths = if kind == "cursor" {
            if root.exists() {
                Ok(vec![LocalUsageFile {
                    url: root.clone(),
                    root,
                    kind,
                }])
            } else {
                Ok(vec![])
            }
        } else {
            let scan_root = if kind == "antigravity" && root.join("conversations").exists() {
                root.join("conversations")
            } else {
                root.clone()
            };
            discover(&scan_root, kind)
        };
        match paths {
            Ok(paths) => {
                for file in paths {
                    let key = file.url.to_string_lossy().into_owned();
                    if seen.insert(key) {
                        discovered.push(file);
                    }
                }
            }
            Err(()) => failures += 1,
        }
    }
    let mut provider_files = Vec::new();
    let mut sources = Vec::new();
    let mut candidates = Vec::new();
    let mut malformed = 0;
    let mut updated: Option<chrono::DateTime<chrono::Utc>> = None;
    for file in &discovered {
        let key = file.url.to_string_lossy().into_owned();
        retained.insert(key.clone());
        let (read, failed) = cache.read(&file.url, || read_file(file));
        failures += usize::from(failed);
        let Some(read) = read else {
            continue;
        };
        malformed += read.malformed;
        if let Some(value) = read.updated {
            updated = Some(updated.map_or(value, |old| old.max(value)));
        }
        if kind == "cursor" {
            for account in read.accounts {
                let path = file.url.join(&account.id).to_string_lossy().into_owned();
                sources.push(Source {
                    kind: kind.into(),
                    directory: path.clone(),
                    volume_id: local_volume_id(&file.url),
                });
                provider_files.push(File {
                    path,
                    kind: kind.into(),
                    modified_ms: account.updated.timestamp_millis() as f64,
                    malformed: 0,
                    records: account.records,
                });
            }
            if read.malformed > 0 {
                let path = file.url.join("invalid").to_string_lossy().into_owned();
                sources.push(Source {
                    kind: kind.into(),
                    directory: path.clone(),
                    volume_id: local_volume_id(&file.url),
                });
                provider_files.push(File {
                    path,
                    kind: kind.into(),
                    modified_ms: now.timestamp_millis() as f64,
                    malformed: read.malformed,
                    records: vec![],
                });
            }
        } else {
            let index = provider_files.len();
            candidates.extend(
                read.candidates
                    .into_iter()
                    .map(|candidate| (candidate, index)),
            );
            provider_files.push(File {
                path: file.url.to_string_lossy().into_owned(),
                kind: kind.into(),
                modified_ms: now.timestamp_millis() as f64,
                malformed: read.malformed,
                records: read.records,
            });
            let directory = file.root.to_string_lossy().into_owned();
            if !sources
                .iter()
                .any(|source: &Source| source.directory == directory)
            {
                sources.push(Source {
                    kind: kind.into(),
                    directory,
                    volume_id: local_volume_id(&file.root),
                });
            }
        }
    }
    if kind == "antigravity" {
        for (record, index) in crate::antigravity::merge(candidates) {
            provider_files[index].records.push(record);
        }
    }
    if !sources.is_empty() {
        dataset.sources.retain(|source| source.kind != kind);
        dataset.files.retain(|file| file.kind != kind);
    }
    dataset.sources.extend(sources);
    dataset.files.extend(provider_files.iter().cloned());
    status(
        kind,
        now,
        discovered.len(),
        failures,
        malformed,
        updated,
        provider_files.len(),
    )
}

pub(crate) fn merge(
    dataset: &mut Dataset,
    roots: &LocalRoots,
    now: chrono::DateTime<chrono::Utc>,
    cache: &mut LocalCache,
) -> Vec<LocalStatus> {
    let mut retained = HashSet::new();
    let statuses = vec![
        scan_provider(
            dataset,
            "opencode",
            roots.opencode.clone(),
            now,
            cache,
            &mut retained,
        ),
        scan_provider(
            dataset,
            "antigravity",
            roots.antigravity.clone(),
            now,
            cache,
            &mut retained,
        ),
        scan_provider(
            dataset,
            "cursor",
            roots.cursor.clone().into_iter().collect(),
            now,
            cache,
            &mut retained,
        ),
    ];
    cache.files.retain(|path, _| retained.contains(path));
    statuses
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_a_file_during_read_retains_old_data_and_retries() {
        let path = std::env::temp_dir().join(format!(
            "bar-usage-fingerprint-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"a").unwrap();
        let mut cache = LocalCache::default();
        let read = |malformed| LocalUsageRead {
            malformed,
            ..LocalUsageRead::default()
        };
        assert_eq!(cache.read(&path, || Ok(read(1))).0.unwrap().malformed, 1);
        std::fs::write(&path, b"bb").unwrap();
        let (retained, failed) = cache.read(&path, || {
            std::fs::write(&path, b"ccc").unwrap();
            Ok(read(2))
        });
        assert!(failed);
        assert_eq!(retained.unwrap().malformed, 1);
        let (fresh, failed) = cache.read(&path, || Ok(read(3)));
        assert!(!failed);
        assert_eq!(fresh.unwrap().malformed, 3);
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(unix)]
fn local_volume_id(path: &std::path::Path) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|metadata| format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn local_volume_id(_path: &std::path::Path) -> Option<String> {
    None
}
