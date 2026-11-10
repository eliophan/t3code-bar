use crate::{
    attention::{AttentionAlert, AttentionTracker},
    settings::{SettingKey, Settings, SettingsStore},
};
use bar_connection::catalog::EnvironmentMetadata;
use bar_identity::{EnvironmentId, EnvironmentMachineKind, EnvironmentSelection, ThreadRef};
use std::{collections::BTreeMap, io};
mod cache;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StaleReason {
    Offline,
    DiscoveryUnavailable,
    ReadFailed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    NeedsPairing,
    PermissionDenied,
    Revoked,
    UnsupportedRoute,
    Missing,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Section<T> {
    Pending,
    Ready {
        value: T,
        observed_at: i64,
    },
    Stale {
        value: T,
        observed_at: i64,
        reason: StaleReason,
    },
    Unavailable(UnavailableReason),
}
impl<T> Section<T> {
    fn offline(&mut self) {
        let old = std::mem::replace(self, Self::Pending);
        *self = match old {
            Self::Ready { value, observed_at }
            | Self::Stale {
                value, observed_at, ..
            } => Self::Stale {
                value,
                observed_at,
                reason: StaleReason::Offline,
            },
            other => other,
        };
    }
    pub fn failed(&mut self, reason: StaleReason, unavailable: UnavailableReason) {
        let old = std::mem::replace(self, Self::Pending);
        *self = match old {
            Self::Ready { value, observed_at }
            | Self::Stale {
                value, observed_at, ..
            } => Self::Stale {
                value,
                observed_at,
                reason,
            },
            _ => Self::Unavailable(unavailable),
        };
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceStamp {
    pub environment: EnvironmentId,
    pub generation: u64,
    pub principal: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAttempt {
    pub environment: EnvironmentId,
    generation: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttemptStatus {
    Offline,
    NeedsPairing,
    Unsupported,
    PermissionDenied,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopedThread {
    reference: ThreadRef,
    item: bar_threads::ThreadItem,
}
impl ScopedThread {
    pub fn new(environment: &EnvironmentId, item: bar_threads::ThreadItem) -> Option<Self> {
        let thread_id = bar_identity::ThreadId::new(&item.id).ok()?;
        Some(Self {
            reference: ThreadRef {
                environment_id: environment.clone(),
                thread_id,
            },
            item,
        })
    }
    pub fn reference(&self) -> &ThreadRef {
        &self.reference
    }
    pub fn item(&self) -> &bar_threads::ThreadItem {
        &self.item
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSnapshot<U, L> {
    pub stamp: SourceStamp,
    pub threads: Section<Vec<ScopedThread>>,
    pub usage: Section<U>,
    pub limits: Section<L>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source<U, L> {
    pub metadata: EnvironmentMetadata,
    pub machine: EnvironmentMachineKind,
    pub snapshot: Option<SourceSnapshot<U, L>>,
    pub stamp: Option<SourceStamp>,
    pub present: bool,
    pub attempt_status: Option<AttemptStatus>,
    attempt_generation: u64,
    cached_threads: Option<cache::CachedThreads>,
    cached_usage: Option<cache::CachedValue<U>>,
    cached_limits: Option<cache::CachedValue<L>>,
    last_generation: Option<u64>,
    baseline: bool,
    attention: AttentionTracker,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryState {
    Pending,
    Ready,
    Stale,
}

/// A single owner applies discovery, sessions, and selection changes in order.
pub struct EnvironmentHub<U, L> {
    settings: SettingsStore,
    selection: EnvironmentSelection,
    revision: u64,
    discovery: DiscoveryState,
    sources: BTreeMap<EnvironmentId, Source<U, L>>,
}
impl<U, L> EnvironmentHub<U, L> {
    pub fn new(settings: SettingsStore) -> Self {
        let selection = settings.value().environment_selection.clone();
        Self {
            settings,
            selection,
            revision: 0,
            discovery: DiscoveryState::Pending,
            sources: BTreeMap::new(),
        }
    }
    pub fn selection(&self) -> &EnvironmentSelection {
        &self.selection
    }
    pub fn settings(&self) -> &Settings {
        self.settings.value()
    }
    pub fn set_setting(&mut self, key: SettingKey, enabled: bool) -> io::Result<()> {
        self.settings.set(key, enabled)
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn discovery_state(&self) -> &DiscoveryState {
        &self.discovery
    }
    pub fn sources(&self) -> &BTreeMap<EnvironmentId, Source<U, L>> {
        &self.sources
    }
    pub fn selected_missing(&self) -> bool {
        match &self.selection {
            EnvironmentSelection::All => false,
            EnvironmentSelection::One(id) => {
                !self.sources.get(id).is_some_and(|source| source.present)
            }
        }
    }
    pub fn selected_sources(&self) -> impl Iterator<Item = &Source<U, L>> {
        self.sources.values().filter(|s| match &self.selection {
            EnvironmentSelection::All => s.metadata.enabled,
            EnvironmentSelection::One(id) => &s.metadata.id == id,
        })
    }
    pub fn set_selection(&mut self, selection: EnvironmentSelection) -> io::Result<u64> {
        if selection != self.selection {
            self.settings.set_environment_selection(selection.clone())?;
            self.selection = selection;
            self.revision = self.revision.wrapping_add(1);
        }
        Ok(self.revision)
    }
    /// Query changes invalidate view-bound asynchronous results without restarting sources.
    pub fn advance_view_revision(&mut self) -> u64 {
        self.revision = self.revision.wrapping_add(1);
        self.revision
    }
    pub fn apply_discovery(&mut self, result: Result<Vec<EnvironmentMetadata>, ()>) {
        let Ok(entries) = result else {
            self.discovery = DiscoveryState::Stale;
            return;
        };
        self.discovery = DiscoveryState::Ready;
        for source in self.sources.values_mut() {
            source.present = false;
        }
        for metadata in entries {
            let id = metadata.id.clone();
            match self.sources.get_mut(&id) {
                Some(source) => {
                    if source.metadata.routes != metadata.routes {
                        source.attempt_generation = source.attempt_generation.wrapping_add(1);
                        source.stamp = None;
                        source.baseline = true;
                        source.attention = AttentionTracker::default();
                        if let Some(snapshot) = &mut source.snapshot {
                            snapshot.threads.offline();
                            snapshot.usage.offline();
                            snapshot.limits.offline();
                        }
                    }
                    source.present = metadata.enabled;
                    source.metadata = metadata;
                }
                None => {
                    let present = metadata.enabled;
                    self.sources.insert(
                        id,
                        Source {
                            metadata,
                            machine: EnvironmentMachineKind::Server,
                            snapshot: None,
                            stamp: None,
                            present,
                            attempt_status: None,
                            attempt_generation: 0,
                            cached_threads: None,
                            cached_usage: None,
                            cached_limits: None,
                            last_generation: None,
                            baseline: true,
                            attention: AttentionTracker::default(),
                        },
                    );
                }
            }
        }
        for source in self.sources.values_mut().filter(|source| !source.present) {
            source.attempt_generation = source.attempt_generation.wrapping_add(1);
            source.stamp = None;
            source.baseline = true;
            source.attention = AttentionTracker::default();
            if let Some(snapshot) = &mut source.snapshot {
                snapshot.threads.offline();
                snapshot.usage.offline();
                snapshot.limits.offline();
            }
        }
    }
    pub fn begin_source(&mut self, stamp: SourceStamp) -> bool {
        let Some(source) = self.sources.get_mut(&stamp.environment) else {
            return false;
        };
        if !source.present {
            return false;
        }
        if source
            .last_generation
            .is_some_and(|old| stamp.generation <= old)
        {
            return false;
        }
        if source
            .snapshot
            .as_ref()
            .is_some_and(|old| old.stamp.principal != stamp.principal)
        {
            source.snapshot = None;
        }
        if source
            .cached_threads
            .as_ref()
            .is_some_and(|cached| cached.principal != stamp.principal)
        {
            source.cached_threads = None;
        }
        if source
            .cached_usage
            .as_ref()
            .is_some_and(|cached| cached.principal != stamp.principal)
        {
            source.cached_usage = None;
        }
        if source
            .cached_limits
            .as_ref()
            .is_some_and(|cached| cached.principal != stamp.principal)
        {
            source.cached_limits = None;
        }
        cache::release_cached(source, &stamp);
        if let Some(snapshot) = &mut source.snapshot {
            snapshot.threads.offline();
            snapshot.usage.offline();
            snapshot.limits.offline();
        }
        source.last_generation = Some(stamp.generation);
        source.attempt_generation = source.attempt_generation.wrapping_add(1);
        source.attempt_status = None;
        source.stamp = Some(stamp);
        source.baseline = true;
        source.attention = AttentionTracker::default();
        true
    }
    pub fn begin_attempt(&mut self, environment: &EnvironmentId) -> Option<SourceAttempt> {
        let source = self.sources.get_mut(environment)?;
        if !source.present {
            return None;
        }
        source.attempt_generation = source.attempt_generation.wrapping_add(1);
        Some(SourceAttempt {
            environment: environment.clone(),
            generation: source.attempt_generation,
        })
    }
    pub fn attempt_current(&self, attempt: &SourceAttempt) -> bool {
        self.sources
            .get(&attempt.environment)
            .is_some_and(|source| source.present && source.attempt_generation == attempt.generation)
    }
    pub fn fail_attempt(&mut self, attempt: &SourceAttempt, status: AttemptStatus) -> bool {
        if !self.attempt_current(attempt) {
            return false;
        }
        let source = self.sources.get_mut(&attempt.environment).unwrap();
        if matches!(
            status,
            AttemptStatus::NeedsPairing | AttemptStatus::PermissionDenied
        ) {
            source.cached_threads = None;
            source.cached_usage = None;
            source.cached_limits = None;
        }
        source.attempt_status = Some(status);
        true
    }
    pub fn complete_attempt(&mut self, attempt: &SourceAttempt) -> bool {
        if !self.attempt_current(attempt) {
            return false;
        }
        self.sources
            .get_mut(&attempt.environment)
            .unwrap()
            .attempt_status = None;
        true
    }
    pub fn begin_attempt_source(
        &mut self,
        attempt: &mut SourceAttempt,
        principal: String,
    ) -> Option<SourceStamp> {
        if !self.attempt_current(attempt) {
            return None;
        }
        let stamp = self.begin_next_source(&attempt.environment, principal)?;
        attempt.generation = self.sources.get(&attempt.environment)?.attempt_generation;
        Some(stamp)
    }
    pub fn begin_next_source(
        &mut self,
        environment: &EnvironmentId,
        principal: impl Into<String>,
    ) -> Option<SourceStamp> {
        let source = self.sources.get(environment)?;
        let stamp = SourceStamp {
            environment: environment.clone(),
            generation: source.last_generation.unwrap_or(0).wrapping_add(1),
            principal: principal.into(),
        };
        self.begin_source(stamp.clone()).then_some(stamp)
    }
    pub fn set_machine(
        &mut self,
        id: &EnvironmentId,
        setting: Option<EnvironmentMachineKind>,
        descriptor: Option<EnvironmentMachineKind>,
    ) {
        if let Some(source) = self.sources.get_mut(id) {
            source.machine = setting
                .or(descriptor)
                .unwrap_or(EnvironmentMachineKind::Server);
        }
    }
    pub fn selected_threads(&self) -> impl Iterator<Item = &ScopedThread> {
        self.selected_sources()
            .filter_map(|source| source.snapshot.as_ref())
            .flat_map(|snapshot| match &snapshot.threads {
                Section::Ready { value, .. } | Section::Stale { value, .. } => value.as_slice(),
                _ => &[],
            })
    }
    /// Whether the current snapshot of the thread's own environment lists it.
    /// Rows the UI still shows from a stale snapshot count. Replaced, revoked,
    /// forgotten, disabled or missing sources list nothing.
    pub fn lists_thread(&self, thread: &ThreadRef) -> bool {
        self.sources
            .get(&thread.environment_id)
            .filter(|source| source.present)
            .and_then(|source| source.snapshot.as_ref())
            .is_some_and(|snapshot| match &snapshot.threads {
                Section::Ready { value, .. } | Section::Stale { value, .. } => {
                    value.iter().any(|row| row.reference() == thread)
                }
                Section::Pending | Section::Unavailable(_) => false,
            })
    }
    pub fn apply_threads(
        &mut self,
        stamp: &SourceStamp,
        value: Vec<ScopedThread>,
        observed_at: i64,
        muted: bool,
    ) -> Option<Vec<AttentionAlert>> {
        let source = self.sources.get_mut(&stamp.environment)?;
        if source.stamp.as_ref() != Some(stamp)
            || value
                .iter()
                .any(|t| t.reference.environment_id != stamp.environment)
        {
            return None;
        }
        let selected = match &self.selection {
            EnvironmentSelection::All => true,
            EnvironmentSelection::One(id) => *id == stamp.environment,
        };
        let alerts = source.attention.update(
            value
                .iter()
                .map(|t| (t.item.id.as_str(), t.item.title.as_str(), t.item.status)),
            muted || !selected || source.baseline,
        );
        source.baseline = false;
        source.cached_threads = None;
        let snapshot = source.snapshot.get_or_insert_with(|| SourceSnapshot {
            stamp: stamp.clone(),
            threads: Section::Pending,
            usage: Section::Pending,
            limits: Section::Pending,
        });
        snapshot.stamp = stamp.clone();
        snapshot.threads = Section::Ready { value, observed_at };
        Some(alerts)
    }
    pub fn update_sections(
        &mut self,
        stamp: &SourceStamp,
        update: impl FnOnce(&mut Section<U>, &mut Section<L>),
    ) -> bool {
        let Some(source) = self.sources.get_mut(&stamp.environment) else {
            return false;
        };
        if source.stamp.as_ref() != Some(stamp) {
            return false;
        }
        let snapshot = source.snapshot.get_or_insert_with(|| SourceSnapshot {
            stamp: stamp.clone(),
            threads: Section::Pending,
            usage: Section::Pending,
            limits: Section::Pending,
        });
        snapshot.stamp = stamp.clone();
        update(&mut snapshot.usage, &mut snapshot.limits);
        true
    }
    pub fn apply_snapshot(&mut self, snapshot: SourceSnapshot<U, L>) -> Option<Vec<ThreadRef>> {
        let source = self.sources.get_mut(&snapshot.stamp.environment)?;
        if source.stamp.as_ref() != Some(&snapshot.stamp)
            || matches!(&snapshot.threads, Section::Ready { value, .. } if value.iter().any(|t| t.reference.environment_id != snapshot.stamp.environment))
        {
            return None;
        }
        let selected = match &self.selection {
            EnvironmentSelection::All => true,
            EnvironmentSelection::One(id) => *id == snapshot.stamp.environment,
        };
        let mut alerts = Vec::new();
        if let Section::Ready { value, .. } = &snapshot.threads {
            alerts = source
                .attention
                .update(
                    value
                        .iter()
                        .map(|t| (t.item.id.as_str(), t.item.title.as_str(), t.item.status)),
                    !selected || source.baseline,
                )
                .into_iter()
                .filter_map(|alert| {
                    value
                        .iter()
                        .find(|t| t.item.id == alert.thread_id)
                        .map(|t| t.reference.clone())
                })
                .collect();
            source.baseline = false;
        }
        source.snapshot = Some(snapshot);
        Some(alerts)
    }
    pub fn offline(&mut self, stamp: &SourceStamp) -> bool {
        let Some(source) = self.sources.get_mut(&stamp.environment) else {
            return false;
        };
        if source.stamp.as_ref() != Some(stamp) {
            return false;
        }
        if let Some(snapshot) = &mut source.snapshot {
            snapshot.threads.offline();
            snapshot.usage.offline();
            snapshot.limits.offline();
        }
        source.baseline = true;
        source.attention = AttentionTracker::default();
        true
    }
    pub fn revoke(&mut self, stamp: &SourceStamp) -> bool {
        let Some(source) = self.sources.get_mut(&stamp.environment) else {
            return false;
        };
        if source.stamp.as_ref() != Some(stamp) {
            return false;
        }
        source.cached_threads = None;
        source.cached_usage = None;
        source.cached_limits = None;
        source.snapshot = Some(SourceSnapshot {
            stamp: stamp.clone(),
            threads: Section::Unavailable(UnavailableReason::Revoked),
            usage: Section::Unavailable(UnavailableReason::Revoked),
            limits: Section::Unavailable(UnavailableReason::Revoked),
        });
        source.stamp = None;
        source.baseline = true;
        source.attention = AttentionTracker::default();
        true
    }

    /// The user removed Bar's saved access. Every in-flight attempt and stamp
    /// becomes stale, protected data and caches are dropped, and the source
    /// waits for new access with a fresh notification baseline. Metadata and
    /// the generation counter are kept.
    pub fn forget_access(&mut self, environment: &EnvironmentId) -> bool {
        let Some(source) = self.sources.get_mut(environment) else {
            return false;
        };
        source.attempt_generation = source.attempt_generation.wrapping_add(1);
        source.attempt_status = Some(AttemptStatus::NeedsPairing);
        source.stamp = None;
        source.snapshot = None;
        source.cached_threads = None;
        source.cached_usage = None;
        source.cached_limits = None;
        source.baseline = true;
        source.attention = AttentionTracker::default();
        true
    }

    /// Hide an explicitly removed manual source once discovery confirms that
    /// no desktop source owns it. Keep its generations to reject late results.
    pub fn dismiss_missing_source(&mut self, environment: &EnvironmentId) {
        if self.sources.get(environment).is_some_and(|source| !source.present) {
            self.forget_access(environment);
            if let Some(source) = self.sources.get_mut(environment) {
                source.metadata.enabled = false;
            }
        }
    }

    /// Clear only the sections whose scope was revoked. Thread access can
    /// remain valid when diagnostics access expires, so revocation is not a
    /// source-wide reset.
    pub fn unavailable_sections(
        &mut self,
        stamp: &SourceStamp,
        threads: bool,
        usage: bool,
        limits: bool,
        reason: UnavailableReason,
    ) -> bool {
        let Some(source) = self.sources.get_mut(&stamp.environment) else {
            return false;
        };
        if source.stamp.as_ref() != Some(stamp) {
            return false;
        }
        let snapshot = source.snapshot.get_or_insert_with(|| SourceSnapshot {
            stamp: stamp.clone(),
            threads: Section::Pending,
            usage: Section::Pending,
            limits: Section::Pending,
        });
        snapshot.stamp = stamp.clone();
        if threads {
            source.cached_threads = None;
            snapshot.threads = Section::Unavailable(reason.clone());
        }
        if usage {
            source.cached_usage = None;
            snapshot.usage = Section::Unavailable(reason.clone());
        }
        if limits {
            source.cached_limits = None;
            snapshot.limits = Section::Unavailable(reason);
        }
        if threads {
            source.baseline = true;
            source.attention = AttentionTracker::default();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_identity::ThreadId;
    fn id(s: &str) -> EnvironmentId {
        EnvironmentId::new(s).unwrap()
    }
    fn meta(s: &str) -> EnvironmentMetadata {
        EnvironmentMetadata {
            id: id(s),
            label: s.into(),
            enabled: true,
            routes: vec![],
        }
    }
    #[test]
    fn cache_roundtrip_restores_local_offline_and_quarantines_remote() {
        for remote_principal in ["a".repeat(64), "cloud:fixture-principal".to_owned()] {
            let dir = tempfile::tempdir().unwrap();
            let settings_path = dir.path().join("settings.json");
            let cache_path = dir.path().join("cache.json");
            let mut hub: EnvironmentHub<(), ()> =
                EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
            let mut remote = meta("remote");
            remote.routes.push(bar_connection::catalog::Route::Bearer {
                connection_id: "credential-marker".into(),
                http_base_url: Some("https://example.test/?token=credential-marker".into()),
            });
            hub.apply_discovery(Ok(vec![meta("local"), remote]));
            hub.set_machine(&id("remote"), Some(EnvironmentMachineKind::MacMini), None);
            for (name, principal) in [("local", "local"), ("remote", remote_principal.as_str())] {
                let stamp = hub.begin_next_source(&id(name), principal).unwrap();
                hub.apply_threads(
                    &stamp,
                    vec![ScopedThread::new(&id(name), test_item(name)).unwrap()],
                    10,
                    false,
                )
                .unwrap();
            }
            hub.set_selection(bar_identity::EnvironmentSelection::One(id("remote")))
                .unwrap();
            hub.save_cache(&cache_path).unwrap();
            let raw = std::fs::read_to_string(&cache_path).unwrap();
            assert!(!raw.contains("routes"));
            assert!(!raw.contains("credential-marker"));
            drop(hub);
            let mut denied: EnvironmentHub<(), ()> =
                EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
            denied.load_cache(&cache_path).unwrap();
            let attempt = denied.begin_attempt(&id("remote")).unwrap();
            assert!(denied.fail_attempt(&attempt, AttemptStatus::PermissionDenied));
            let denied_path = dir.path().join("denied-cache.json");
            denied.save_cache(&denied_path).unwrap();
            let mut denied_restart: EnvironmentHub<(), ()> =
                EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
            denied_restart.load_cache(&denied_path).unwrap();
            assert!(denied_restart.sources()[&id("remote")]
                .cached_threads
                .is_none());
            let mut restored: EnvironmentHub<(), ()> =
                EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
            restored.load_cache(&cache_path).unwrap();
            assert_eq!(
                restored.selection(),
                &bar_identity::EnvironmentSelection::One(id("remote"))
            );
            assert_eq!(
                restored.sources()[&id("remote")].machine,
                EnvironmentMachineKind::MacMini
            );
            assert_eq!(restored.selected_threads().count(), 0);
            let mut authenticated: EnvironmentHub<(), ()> =
                EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
            authenticated.load_cache(&cache_path).unwrap();
            authenticated
                .begin_next_source(&id("remote"), remote_principal.clone())
                .unwrap();
            assert_eq!(authenticated.selected_threads().count(), 1);
            assert!(matches!(
                authenticated.sources()[&id("remote")]
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .threads,
                Section::Stale { .. }
            ));
            restored
                .set_selection(bar_identity::EnvironmentSelection::All)
                .unwrap();
            assert_eq!(restored.selected_sources().count(), 2);
            let local = restored.begin_next_source(&id("local"), "local").unwrap();
            assert_eq!(
                restored.selected_threads().next().unwrap().item().title,
                "local"
            );
            assert!(matches!(
                restored.sources()[&id("local")]
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .threads,
                Section::Stale { .. }
            ));
            assert!(restored
                .apply_threads(
                    &local,
                    vec![ScopedThread::new(&id("local"), test_item("local")).unwrap()],
                    11,
                    false
                )
                .unwrap()
                .is_empty());
            let changed = restored
                .begin_next_source(&id("remote"), "b".repeat(64))
                .unwrap();
            assert_eq!(restored.selected_threads().count(), 1);
            assert!(restored.revoke(&changed));
            restored.save_cache(&cache_path).unwrap();
            let mut again: EnvironmentHub<(), ()> =
                EnvironmentHub::new(SettingsStore::load(settings_path).unwrap());
            again.load_cache(&cache_path).unwrap();
            assert!(again.sources()[&id("remote")].cached_threads.is_none());
        }
    }

    #[test]
    fn cache_rejects_corruption_and_stale_attempts_cannot_change_new_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache");
        let mut hub: EnvironmentHub<(), ()> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        std::fs::write(&path, br#"{"version":99,"sources":[]}"#).unwrap();
        assert!(hub.load_cache(&path).is_err());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            br#"{"version":99,"sources":[]}"#
        );
        std::fs::write(&path, b"broken").unwrap();
        assert!(hub.load_cache(&path).is_err());
        hub.apply_discovery(Ok(vec![meta("remote")]));
        let old = hub.begin_attempt(&id("remote")).unwrap();
        let current = hub.begin_attempt(&id("remote")).unwrap();
        assert!(!hub.fail_attempt(&old, AttemptStatus::NeedsPairing));
        assert!(hub.fail_attempt(&current, AttemptStatus::Unsupported));
        let stamp = hub.begin_next_source(&id("remote"), "principal").unwrap();
        assert!(!hub.fail_attempt(&current, AttemptStatus::PermissionDenied));
        assert_eq!(hub.sources()[&id("remote")].stamp.as_ref(), Some(&stamp));
        assert_eq!(hub.sources()[&id("remote")].attempt_status, None);
    }

    #[test]
    fn cache_restores_last_good_usage_and_limits_only_for_same_principal() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings");
        let path = dir.path().join("cache");
        let mut hub: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
        let environment = id("remote");
        hub.apply_discovery(Ok(vec![meta("remote")]));
        let stamp = hub.begin_next_source(&environment, "cloud:opaque").unwrap();
        hub.apply_threads(&stamp, vec![], 10, true);
        assert!(hub.update_sections(&stamp, |usage, limits| {
            *usage = Section::Ready {
                value: "usage".into(),
                observed_at: 11,
            };
            *limits = Section::Ready {
                value: "limits".into(),
                observed_at: 12,
            };
        }));
        hub.save_cache(&path).unwrap();

        let mut denied: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
        denied.load_cache(&path).unwrap();
        let attempt = denied.begin_attempt(&environment).unwrap();
        assert!(denied.fail_attempt(&attempt, AttemptStatus::PermissionDenied));
        let denied_path = dir.path().join("denied-cache");
        denied.save_cache(&denied_path).unwrap();
        let mut denied_restart: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
        denied_restart.load_cache(&denied_path).unwrap();
        denied_restart
            .begin_next_source(&environment, "cloud:opaque")
            .unwrap();
        assert!(denied_restart.sources()[&environment].snapshot.is_none());

        let mut restored: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(settings_path.clone()).unwrap());
        restored.load_cache(&path).unwrap();
        let same = restored
            .begin_next_source(&environment, "cloud:opaque")
            .unwrap();
        let snapshot = restored.sources()[&environment].snapshot.as_ref().unwrap();
        assert!(matches!(&snapshot.usage, Section::Stale { value, .. } if value == "usage"));
        assert!(matches!(&snapshot.limits, Section::Stale { value, .. } if value == "limits"));

        let mut different: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(settings_path).unwrap());
        different.load_cache(&path).unwrap();
        let changed = different
            .begin_next_source(&environment, "cloud:other")
            .unwrap();
        let snapshot = different.sources()[&environment].snapshot.as_ref();
        assert!(snapshot.is_none_or(|snapshot| {
            matches!(snapshot.usage, Section::Pending)
                && matches!(snapshot.limits, Section::Pending)
        }));
        assert_eq!(same.principal, "cloud:opaque");
        assert_eq!(changed.principal, "cloud:other");
    }
    fn stamp(s: &str, generation: u64, principal: &str) -> SourceStamp {
        SourceStamp {
            environment: id(s),
            generation,
            principal: principal.into(),
        }
    }
    fn test_item(id: &str) -> bar_threads::ThreadItem {
        serde_json::from_value(serde_json::json!({
            "id":id, "projectId":"p", "projectTitle":"p", "title":id,
            "createdAt":"2024-01-01T00:00:00Z", "updatedAt":"2024-01-01T00:00:00Z",
            "pendingUserInputCount":0, "pendingApprovalCount":0, "providers":[],
            "hasGoal":false, "isPinned":false, "isSettled":false, "isSnoozed":false,
            "isWoke":false, "pendingGuestMessageCount":0, "isBlockedOnThreadWait":false,
            "hasRuns":false, "status":"Approval", "priority":0, "isRunning":false,
            "isAttention":true, "showsLabelInMenuBar":true
        }))
        .unwrap()
    }
    fn snapshot(stamp: SourceStamp, threads: &[&str]) -> SourceSnapshot<String, String> {
        SourceSnapshot {
            threads: Section::Ready {
                value: threads
                    .iter()
                    .map(|t| ScopedThread::new(&stamp.environment, test_item(t)).unwrap())
                    .collect(),
                observed_at: 10,
            },
            usage: Section::Ready {
                value: "usage".into(),
                observed_at: 10,
            },
            limits: Section::Unavailable(UnavailableReason::PermissionDenied),
            stamp,
        }
    }
    #[test]
    fn contribution_selection_deduplicates_and_retains_offline_data() {
        use bar_identity::EnvironmentSelection;
        use bar_usage::{
            merge_usage, EnvironmentUsage, UsageContribution, UsageQuery, USAGE_CONTRACT_VERSION,
        };
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<UsageContribution, bar_limits::LimitsContribution> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("a"), meta("b")]));
        let a = stamp("a", 1, "a");
        let b = stamp("b", 1, "b");
        assert!(hub.begin_source(a.clone()));
        assert!(hub.begin_source(b.clone()));
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/multi-environment/usage/parity.json"
        ))
        .unwrap();
        let mut environment: EnvironmentUsage =
            serde_json::from_value(corpus[0]["environments"][0].clone()).unwrap();
        let query = UsageQuery {
            since_day: "2026-09-01".into(),
            until_day: "2026-09-30".into(),
            time_zone: "UTC".into(),
            resolution: "day".into(),
            since_time: None,
            until_time: None,
            model_filter: None,
        };
        for source in [&a, &b] {
            environment.environment_id = source.environment.clone();
            let contribution = UsageContribution::bind(query.clone(), environment.clone()).unwrap();
            assert!(
                hub.update_sections(source, |usage, _| *usage = Section::Ready {
                    value: contribution,
                    observed_at: 1
                })
            );
        }
        let values = |hub: &EnvironmentHub<UsageContribution, bar_limits::LimitsContribution>| {
            hub.selected_sources()
                .filter_map(|s| match &s.snapshot.as_ref()?.usage {
                    Section::Ready { value, .. } | Section::Stale { value, .. } => {
                        Some(value.clone())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let merged = merge_usage(
            &values(&hub),
            &query,
            hub.selection(),
            USAGE_CONTRACT_VERSION,
        );
        assert_eq!(merged.buckets.len(), 1);
        assert_eq!(merged.duplicate_sources.len(), 1);
        hub.set_selection(EnvironmentSelection::One(id("b")))
            .unwrap();
        assert_eq!(
            merge_usage(
                &values(&hub),
                &query,
                hub.selection(),
                USAGE_CONTRACT_VERSION
            )
            .buckets
            .len(),
            1
        );
        assert!(hub.offline(&b));
        assert!(matches!(
            hub.sources()[&id("b")].snapshot.as_ref().unwrap().usage,
            Section::Stale { .. }
        ));
        assert_eq!(values(&hub).len(), 1);
        let mut other = query.clone();
        other.resolution = "hour".into();
        assert!(merge_usage(
            &values(&hub),
            &other,
            hub.selection(),
            USAGE_CONTRACT_VERSION
        )
        .buckets
        .is_empty());
        let next = stamp("b", 2, "b");
        assert!(hub.begin_source(next));
        assert!(!hub.update_sections(&b, |usage, _| *usage = Section::Pending));
        assert_eq!(values(&hub).len(), 1);
    }
    #[test]
    fn selected_limits_reconcile_accounts_and_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<(), bar_limits::LimitsContribution> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/multi-environment/limits/parity.json"
        ))
        .unwrap();
        let case = &corpus["cases"][0];
        let envs = case["envs"].as_array().unwrap();
        hub.apply_discovery(Ok(envs
            .iter()
            .map(|e| meta(e["id"].as_str().unwrap()))
            .collect()));
        for env in envs {
            let name = env["id"].as_str().unwrap();
            let stamp = stamp(name, 1, name);
            assert!(hub.begin_source(stamp.clone()));
            let mut parser = bar_limits::LiveLimitsParser::default();
            parser.apply(&serde_json::json!({"version":1,"type":"snapshot","config":{"providers":env["providers"],"usageLimitSources":env["sources"]}})).unwrap();
            let contribution = parser.contribution(id(name));
            assert!(
                hub.update_sections(&stamp, |_, limits| *limits = Section::Ready {
                    value: contribution,
                    observed_at: 1
                })
            );
        }
        let contributions = hub
            .selected_sources()
            .filter_map(|s| match &s.snapshot.as_ref()?.limits {
                Section::Ready { value, .. } => Some(value.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let merged = bar_limits::merge_contributions(&contributions, hub.selection());
        assert_eq!(merged.accounts.len(), 1);
        assert_eq!(
            merged.accounts[0].windows[0].used_percent,
            case["upstream"]["windows"][0][0].as_f64().unwrap()
        );
        hub.set_selection(EnvironmentSelection::One(id(envs[0]["id"]
            .as_str()
            .unwrap())))
            .unwrap();
        let selected = bar_limits::merge_contributions(&contributions, hub.selection());
        assert_eq!(selected.accounts.len(), 1);
    }
    #[test]
    fn thread_updates_leave_usage_and_limits_sections_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("a")]));
        let stamp = stamp("a", 1, "a");
        assert!(hub.begin_source(stamp.clone()));
        let thread = ScopedThread::new(&id("a"), test_item("same")).unwrap();
        hub.apply_snapshot(SourceSnapshot {
            stamp: stamp.clone(),
            threads: Section::Ready {
                value: vec![thread.clone()],
                observed_at: 1,
            },
            usage: Section::Ready {
                value: "usage".into(),
                observed_at: 1,
            },
            limits: Section::Unavailable(UnavailableReason::PermissionDenied),
        });
        hub.apply_threads(&stamp, vec![thread], 2, false).unwrap();
        let snapshot = hub.sources()[&id("a")].snapshot.as_ref().unwrap();
        assert!(matches!(snapshot.usage, Section::Ready { ref value, .. } if value == "usage"));
        assert!(matches!(
            snapshot.limits,
            Section::Unavailable(UnavailableReason::PermissionDenied)
        ));
    }
    #[test]
    fn selected_view_keeps_scoped_payloads_and_stale_unselected_sources() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<(), ()> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("a"), meta("b")]));
        let a = stamp("a", 1, "a");
        let b = stamp("b", 1, "b");
        assert!(hub.begin_source(a.clone()));
        assert!(hub.begin_source(b.clone()));
        let a_thread = ScopedThread::new(&id("a"), test_item("same")).unwrap();
        let mut b_item = test_item("same");
        b_item.title = "B title".into();
        let b_thread = ScopedThread::new(&id("b"), b_item).unwrap();
        assert!(hub
            .apply_threads(&a, vec![a_thread], 1, false)
            .unwrap()
            .is_empty());
        assert!(hub
            .apply_threads(&b, vec![b_thread], 1, false)
            .unwrap()
            .is_empty());
        assert_eq!(hub.selected_threads().count(), 2);
        assert!(hub
            .selected_threads()
            .any(|thread| thread.item().title == "B title"));
        hub.set_selection(EnvironmentSelection::One(id("a")))
            .unwrap();
        assert_eq!(hub.selected_threads().count(), 1);
        hub.offline(&b);
        hub.set_selection(EnvironmentSelection::All).unwrap();
        assert_eq!(hub.selected_threads().count(), 2);
        assert!(matches!(
            hub.sources()[&id("b")].snapshot.as_ref().unwrap().threads,
            Section::Stale { .. }
        ));
    }
    #[test]
    fn selection_discovery_and_session_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let settings = SettingsStore::load(dir.path().join("settings.json")).unwrap();
        let mut hub = EnvironmentHub::new(settings);
        hub.apply_discovery(Ok(vec![meta("a"), meta("b")]));
        let a = stamp("a", 1, "alice");
        assert!(hub.begin_source(a.clone()));
        assert_eq!(
            hub.apply_snapshot(snapshot(a.clone(), &["1"])),
            Some(vec![])
        );
        assert_eq!(
            hub.apply_snapshot(snapshot(a.clone(), &["1", "2"]))
                .unwrap()
                .len(),
            1
        );
        hub.set_selection(EnvironmentSelection::One(id("b")))
            .unwrap();
        assert!(hub
            .apply_snapshot(snapshot(a.clone(), &["1", "2", "3"]))
            .unwrap()
            .is_empty());
        hub.apply_discovery(Err(()));
        assert_eq!(hub.discovery_state(), &DiscoveryState::Stale);
        assert!(!hub.selected_missing());
        hub.apply_discovery(Ok(vec![meta("a")]));
        assert!(hub.selected_missing());
        assert_eq!(hub.selection(), &EnvironmentSelection::One(id("b")));
        assert!(hub.offline(&a));
        assert!(matches!(
            hub.sources()[&id("a")].snapshot.as_ref().unwrap().usage,
            Section::Stale { .. }
        ));
        let next = stamp("a", 2, "bob");
        assert!(hub.begin_source(next.clone()));
        assert!(hub.sources()[&id("a")].snapshot.is_none());
        assert!(hub.apply_snapshot(snapshot(a.clone(), &["late"])).is_none());
        assert!(hub
            .apply_snapshot(snapshot(next.clone(), &["1"]))
            .unwrap()
            .is_empty());
        assert!(hub.revoke(&next));
        assert!(!hub.begin_source(next.clone()));
        assert!(hub.apply_snapshot(snapshot(next, &["late"])).is_none());
        assert!(matches!(
            hub.sources()[&id("a")].snapshot.as_ref().unwrap().usage,
            Section::Unavailable(UnavailableReason::Revoked)
        ));
    }
    #[test]
    fn failed_save_does_not_publish_selection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let store = SettingsStore::load(path.clone()).unwrap();
        let mut hub: EnvironmentHub<(), ()> = EnvironmentHub::new(store);
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(path).unwrap();
        assert!(hub
            .set_selection(EnvironmentSelection::One(id("x")))
            .is_err());
        assert_eq!(hub.selection(), &EnvironmentSelection::All);
        assert_eq!(hub.revision(), 0);
    }
    #[test]
    fn removal_and_disable_reject_late_sessions_and_keep_stale_data() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("a")]));
        let first = stamp("a", 1, "alice");
        assert!(hub.begin_source(first.clone()));
        hub.apply_snapshot(snapshot(first.clone(), &["one"]));
        hub.apply_discovery(Ok(vec![]));
        assert_eq!(hub.selected_threads().count(), 1);
        assert!(hub
            .apply_snapshot(snapshot(first.clone(), &["one", "late"]))
            .is_none());
        assert!(matches!(
            hub.sources()[&id("a")].snapshot.as_ref().unwrap().threads,
            Section::Stale { .. }
        ));
        assert!(!hub.begin_source(stamp("a", 2, "alice")));
        let mut disabled = meta("a");
        disabled.enabled = false;
        hub.apply_discovery(Ok(vec![disabled]));
        assert!(!hub.begin_source(stamp("a", 2, "alice")));
        hub.apply_discovery(Ok(vec![meta("a")]));
        assert!(!hub.begin_source(first));
        assert!(hub.begin_source(stamp("a", 2, "alice")));
    }
    #[test]
    fn dismissing_manual_source_rejects_late_data_and_can_be_added_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("manual")]));
        let first = stamp("manual", 1, "alice");
        assert!(hub.begin_source(first.clone()));
        hub.apply_snapshot(snapshot(first.clone(), &["one"]));
        let pending = hub.begin_attempt(&id("manual")).unwrap();
        hub.apply_discovery(Ok(vec![]));
        hub.dismiss_missing_source(&id("manual"));
        assert_eq!(hub.selected_threads().count(), 0);
        assert!(!hub.sources()[&id("manual")].metadata.enabled);
        assert!(!hub.attempt_current(&pending));
        assert!(hub.apply_snapshot(snapshot(first.clone(), &["late"])).is_none());
        hub.apply_discovery(Ok(vec![meta("manual")]));
        assert!(!hub.begin_source(first));
        let fresh = hub.begin_next_source(&id("manual"), "alice").unwrap();
        assert!(hub.apply_snapshot(snapshot(fresh, &["new"])).is_some());
        assert_eq!(hub.selected_threads().count(), 1);
    }
    #[test]
    fn selection_preserves_seen_history_and_reports_new_selected_threads() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("a"), meta("b")]));
        let a = stamp("a", 1, "alice");
        assert!(hub.begin_source(a.clone()));
        assert!(hub
            .apply_snapshot(snapshot(a.clone(), &["one"]))
            .unwrap()
            .is_empty());
        hub.set_selection(EnvironmentSelection::One(id("b")))
            .unwrap();
        assert!(hub
            .apply_snapshot(snapshot(a.clone(), &["one", "two"]))
            .unwrap()
            .is_empty());
        hub.set_selection(EnvironmentSelection::One(id("a")))
            .unwrap();
        assert!(hub
            .apply_snapshot(snapshot(a.clone(), &["one", "two"]))
            .unwrap()
            .is_empty());
        let alerts = hub
            .apply_snapshot(snapshot(a, &["one", "two", "three"]))
            .unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].thread_id, ThreadId::new("three").unwrap());
    }
}

#[cfg(test)]
mod remote_runtime_tests {
    use super::*;
    use bar_identity::EnvironmentSelection;

    fn id(value: &str) -> EnvironmentId {
        EnvironmentId::new(value).unwrap()
    }
    fn meta(value: &str) -> EnvironmentMetadata {
        EnvironmentMetadata {
            id: id(value),
            label: value.into(),
            enabled: true,
            routes: vec![],
        }
    }
    fn test_item(value: &str) -> bar_threads::ThreadItem {
        serde_json::from_value(serde_json::json!({
            "id": value, "projectId":"p", "projectTitle":"p", "title":value,
            "createdAt":"2024-01-01T00:00:00Z", "updatedAt":"2024-01-01T00:00:00Z",
            "pendingUserInputCount":0, "pendingApprovalCount":0, "providers":[],
            "hasGoal":false, "isPinned":false, "isSettled":false, "isSnoozed":false,
            "isWoke":false, "pendingGuestMessageCount":0, "isBlockedOnThreadWait":false,
            "hasRuns":false, "status":"Approval", "priority":0, "isRunning":false,
            "isAttention":true, "showsLabelInMenuBar":true
        }))
        .unwrap()
    }
    fn stamp(value: &str, generation: u64, principal: &str) -> SourceStamp {
        SourceStamp {
            environment: id(value),
            generation,
            principal: principal.into(),
        }
    }

    fn thread_ref(environment: &str, thread: &str) -> ThreadRef {
        ThreadRef {
            environment_id: id(environment),
            thread_id: bar_identity::ThreadId::new(thread).unwrap(),
        }
    }

    #[test]
    fn listed_threads_follow_the_current_snapshot_per_environment() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("a"), meta("b")]));
        let a = stamp("a", 1, "grant-a");
        let b = stamp("b", 1, "grant-b");
        assert!(hub.begin_source(a.clone()) && hub.begin_source(b.clone()));
        let rows = |stamp: &SourceStamp, ids: &[&str]| -> Vec<ScopedThread> {
            ids.iter()
                .map(|value| ScopedThread::new(&stamp.environment, test_item(value)).unwrap())
                .collect()
        };
        assert!(
            !hub.lists_thread(&thread_ref("a", "one")),
            "nothing is listed before a snapshot"
        );
        hub.apply_threads(&a, rows(&a, &["one", "same"]), 1, true);
        hub.apply_threads(&b, rows(&b, &["same"]), 1, true);
        assert!(hub.lists_thread(&thread_ref("a", "one")));
        assert!(hub.lists_thread(&thread_ref("a", "same")));
        assert!(hub.lists_thread(&thread_ref("b", "same")));
        assert!(
            !hub.lists_thread(&thread_ref("b", "one")),
            "an ID listed by another environment does not authorize this one"
        );
        assert!(!hub.lists_thread(&thread_ref("missing", "same")));

        hub.apply_threads(&a, rows(&a, &["same"]), 2, true);
        assert!(
            !hub.lists_thread(&thread_ref("a", "one")),
            "a thread that left the listing is no longer authorized"
        );
        assert!(hub.lists_thread(&thread_ref("a", "same")));
        assert!(hub.lists_thread(&thread_ref("b", "same")));

        hub.offline(&b);
        assert!(
            hub.lists_thread(&thread_ref("b", "same")),
            "rows shown from a stale snapshot stay actionable"
        );

        hub.revoke(&b);
        assert!(!hub.lists_thread(&thread_ref("b", "same")));
        hub.forget_access(&a.environment);
        assert!(!hub.lists_thread(&thread_ref("a", "same")));
    }

    #[test]
    fn disabled_and_unavailable_sources_list_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("a"), meta("b")]));
        let a = stamp("a", 1, "grant-a");
        let b = stamp("b", 1, "grant-b");
        assert!(hub.begin_source(a.clone()) && hub.begin_source(b.clone()));
        for stamp in [&a, &b] {
            let row = ScopedThread::new(&stamp.environment, test_item("same")).unwrap();
            hub.apply_threads(stamp, vec![row], 1, true);
        }
        hub.unavailable_sections(&a, true, false, false, UnavailableReason::PermissionDenied);
        assert!(!hub.lists_thread(&thread_ref("a", "same")));
        hub.apply_discovery(Ok(vec![meta("a")]));
        assert!(
            !hub.lists_thread(&thread_ref("b", "same")),
            "an environment discovery no longer reports lists nothing"
        );
    }

    #[test]
    fn remote_payloads_are_scoped_and_permission_revocation_clears_protected_sections() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("remote-a"), meta("remote-b")]));
        let a = stamp("remote-a", 1, "grant-a");
        let b = stamp("remote-b", 1, "grant-b");
        assert!(hub.begin_source(a.clone()));
        assert!(hub.begin_source(b.clone()));
        let a_thread = ScopedThread::new(&a.environment, test_item("same-thread-id")).unwrap();
        let mut b_item = test_item("same-thread-id");
        b_item.title = "Remote B".into();
        let b_thread = ScopedThread::new(&b.environment, b_item).unwrap();
        assert!(hub.apply_threads(&a, vec![a_thread], 10, true).is_some());
        assert!(hub.apply_threads(&b, vec![b_thread], 10, true).is_some());
        assert_eq!(hub.selected_threads().count(), 2);
        hub.set_selection(EnvironmentSelection::One(id("remote-b")))
            .unwrap();
        assert_eq!(hub.selected_threads().count(), 1);
        assert_eq!(
            hub.selected_threads().next().unwrap().item().title,
            "Remote B"
        );
        assert!(hub.update_sections(&b, |usage, limits| {
            *usage = Section::Ready {
                value: "usage".into(),
                observed_at: 10,
            };
            *limits = Section::Ready {
                value: "limits".into(),
                observed_at: 10,
            };
        }));
        assert!(hub.offline(&b));
        assert!(
            matches!(hub.sources()[&id("remote-b")].snapshot.as_ref().unwrap().usage, Section::Stale { ref value, .. } if value == "usage")
        );
        assert!(hub.unavailable_sections(
            &b,
            false,
            true,
            true,
            UnavailableReason::PermissionDenied
        ));
        let snapshot = hub.sources()[&id("remote-b")].snapshot.as_ref().unwrap();
        assert!(matches!(snapshot.threads, Section::Stale { .. }));
        assert!(matches!(
            snapshot.usage,
            Section::Unavailable(UnavailableReason::PermissionDenied)
        ));
        assert!(matches!(
            snapshot.limits,
            Section::Unavailable(UnavailableReason::PermissionDenied)
        ));
        let next = stamp("remote-b", 2, "grant-b");
        assert!(hub.begin_source(next));
        assert!(!hub.apply_threads(&b, Vec::new(), 11, true).is_some());
    }

    #[test]
    fn forgetting_access_invalidates_attempts_and_data_for_that_source_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub: EnvironmentHub<String, String> =
            EnvironmentHub::new(SettingsStore::load(dir.path().join("settings")).unwrap());
        hub.apply_discovery(Ok(vec![meta("remote-a"), meta("remote-b")]));
        let a = stamp("remote-a", 1, "grant-a");
        let b = stamp("remote-b", 1, "grant-b");
        assert!(hub.begin_source(a.clone()));
        assert!(hub.begin_source(b.clone()));
        for stamp in [&a, &b] {
            let thread = ScopedThread::new(&stamp.environment, test_item("t")).unwrap();
            hub.apply_threads(stamp, vec![thread], 10, true).unwrap();
        }
        let queued = hub.begin_attempt(&id("remote-a")).unwrap();
        let other = hub.begin_attempt(&id("remote-b")).unwrap();
        hub.set_selection(EnvironmentSelection::One(id("remote-a")))
            .unwrap();

        assert!(hub.forget_access(&id("remote-a")));
        assert!(!hub.forget_access(&id("missing")));
        assert!(!hub.attempt_current(&queued));
        assert!(!hub.complete_attempt(&queued));
        assert!(hub.attempt_current(&other));
        let source = &hub.sources()[&id("remote-a")];
        assert!(source.stamp.is_none() && source.snapshot.is_none());
        assert_eq!(source.attempt_status, Some(AttemptStatus::NeedsPairing));
        assert!(source.present);
        assert_eq!(hub.selection(), &EnvironmentSelection::One(id("remote-a")));
        assert_eq!(hub.selected_threads().count(), 0);
        assert!(hub.apply_threads(&a, Vec::new(), 11, false).is_none());
        assert!(hub.sources()[&id("remote-b")].snapshot.is_some());

        let renewed = hub.begin_next_source(&id("remote-a"), "grant-new").unwrap();
        assert_eq!(renewed.generation, 2);
        let alerts = hub
            .apply_threads(
                &renewed,
                vec![ScopedThread::new(&renewed.environment, test_item("t")).unwrap()],
                12,
                false,
            )
            .unwrap();
        assert!(
            alerts.is_empty(),
            "first read after re-pairing is a baseline"
        );
    }
}
