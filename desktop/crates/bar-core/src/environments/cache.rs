use super::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::Path,
};

const VERSION: u32 = 2;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SOURCES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CachedThreads {
    pub principal: String,
    observed_at: i64,
    items: Vec<bar_threads::ThreadItem>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CachedValue<T> {
    pub principal: String,
    observed_at: i64,
    pub value: T,
}
#[derive(Serialize, Deserialize)]
struct CachedSource<U, L> {
    id: EnvironmentId,
    label: String,
    machine: EnvironmentMachineKind,
    present: bool,
    threads: Option<CachedThreads>,
    usage: Option<CachedValue<U>>,
    limits: Option<CachedValue<L>>,
}
#[derive(Serialize, Deserialize)]
struct Cache<U, L> {
    version: u32,
    sources: Vec<CachedSource<U, L>>,
}

fn valid_principal(principal: &str) -> bool {
    principal == "local"
        || principal.strip_prefix("cloud:").is_some_and(|id| {
            !id.is_empty()
                && id.len() <= 256
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        })
        || (principal.len() == 64
            && principal
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

pub(super) fn release_cached<U, L>(source: &mut Source<U, L>, stamp: &SourceStamp) {
    let cached_threads = source
        .cached_threads
        .take()
        .filter(|cached| cached.principal == stamp.principal);
    let cached_usage = source
        .cached_usage
        .take()
        .filter(|cached| cached.principal == stamp.principal);
    let cached_limits = source
        .cached_limits
        .take()
        .filter(|cached| cached.principal == stamp.principal);
    if cached_threads.is_none() && cached_usage.is_none() && cached_limits.is_none() {
        return;
    }
    let threads = cached_threads
        .map(|cached| Section::Stale {
            value: cached
                .items
                .into_iter()
                .filter_map(|item| ScopedThread::new(&stamp.environment, item))
                .collect(),
            observed_at: cached.observed_at,
            reason: StaleReason::Offline,
        })
        .unwrap_or(Section::Pending);
    let usage = cached_usage
        .map(|cached| Section::Stale {
            value: cached.value,
            observed_at: cached.observed_at,
            reason: StaleReason::Offline,
        })
        .unwrap_or(Section::Pending);
    let limits = cached_limits
        .map(|cached| Section::Stale {
            value: cached.value,
            observed_at: cached.observed_at,
            reason: StaleReason::Offline,
        })
        .unwrap_or(Section::Pending);
    source.snapshot = Some(SourceSnapshot {
        stamp: stamp.clone(),
        threads,
        usage,
        limits,
    });
}

fn cached_value<T: Clone>(principal: &str, section: &Section<T>) -> Option<CachedValue<T>> {
    if !valid_principal(principal) {
        return None;
    }
    match section {
        Section::Ready { value, observed_at }
        | Section::Stale {
            value, observed_at, ..
        } => Some(CachedValue {
            principal: principal.to_owned(),
            observed_at: *observed_at,
            value: value.clone(),
        }),
        _ => None,
    }
}

impl<U, L> EnvironmentHub<U, L> {
    pub fn load_cache(&mut self, path: &Path) -> io::Result<()>
    where
        U: DeserializeOwned,
        L: DeserializeOwned,
    {
        let mut data = Vec::new();
        match fs::File::open(path) {
            Ok(file) => {
                file.take(MAX_BYTES + 1).read_to_end(&mut data)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
        if data.len() as u64 > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "environment cache is too large",
            ));
        }
        let cache: Cache<U, L> = serde_json::from_slice(&data)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let mut ids = BTreeSet::new();
        if cache.version != VERSION
            || cache.sources.len() > MAX_SOURCES
            || cache.sources.iter().any(|source| {
                !ids.insert(source.id.clone())
                    || source.label.len() > 4096
                    || source.threads.as_ref().is_some_and(|threads| {
                        !valid_principal(&threads.principal)
                            || threads.items.len() > 10000
                            || threads
                                .items
                                .iter()
                                .any(|item| ScopedThread::new(&source.id, item.clone()).is_none())
                    })
                    || source
                        .usage
                        .as_ref()
                        .is_some_and(|usage| !valid_principal(&usage.principal))
                    || source
                        .limits
                        .as_ref()
                        .is_some_and(|limits| !valid_principal(&limits.principal))
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid environment cache",
            ));
        }
        for cached in cache.sources {
            self.sources.insert(
                cached.id.clone(),
                Source {
                    metadata: EnvironmentMetadata {
                        id: cached.id,
                        label: cached.label,
                        enabled: true,
                        routes: vec![],
                    },
                    machine: cached.machine,
                    snapshot: None,
                    stamp: None,
                    present: cached.present,
                    attempt_status: Some(AttemptStatus::Offline),
                    attempt_generation: 0,
                    cached_threads: cached.threads,
                    cached_usage: cached.usage,
                    cached_limits: cached.limits,
                    last_generation: None,
                    baseline: true,
                    attention: AttentionTracker::default(),
                },
            );
        }
        self.discovery = DiscoveryState::Stale;
        Ok(())
    }

    pub fn save_cache(&self, path: &Path) -> io::Result<()>
    where
        U: Serialize + Clone,
        L: Serialize + Clone,
    {
        let mut sources = Vec::new();
        for source in self
            .sources
            .values()
            .filter(|source| source.metadata.enabled)
            .take(MAX_SOURCES)
        {
            let threads = source
                .snapshot
                .as_ref()
                .and_then(|snapshot| {
                    if !valid_principal(&snapshot.stamp.principal) {
                        return None;
                    }
                    match &snapshot.threads {
                        Section::Ready { value, observed_at }
                        | Section::Stale {
                            value, observed_at, ..
                        } if value.len() <= 10000 => Some(CachedThreads {
                            principal: snapshot.stamp.principal.clone(),
                            observed_at: *observed_at,
                            items: value.iter().map(|thread| thread.item.clone()).collect(),
                        }),
                        _ => None,
                    }
                })
                .or_else(|| source.cached_threads.clone());
            let usage = source
                .snapshot
                .as_ref()
                .and_then(|snapshot| cached_value(&snapshot.stamp.principal, &snapshot.usage))
                .or_else(|| source.cached_usage.clone());
            let limits = source
                .snapshot
                .as_ref()
                .and_then(|snapshot| cached_value(&snapshot.stamp.principal, &snapshot.limits))
                .or_else(|| source.cached_limits.clone());
            sources.push(CachedSource {
                id: source.metadata.id.clone(),
                label: source.metadata.label.clone(),
                machine: source.machine,
                present: source.present,
                threads,
                usage,
                limits,
            });
        }
        let data = serde_json::to_vec(&Cache {
            version: VERSION,
            sources,
        })?;
        if data.len() as u64 > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "environment cache is too large",
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let temporary = path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        let result = file
            .write_all(&data)
            .and_then(|()| file.sync_all())
            .and_then(|()| fs::rename(&temporary, path));
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        #[cfg(unix)]
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}
