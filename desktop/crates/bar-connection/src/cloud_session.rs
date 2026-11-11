use crate::{
    cloud::{CloudConfig, CloudGrant, PendingLogin},
    credentials::{CredentialScope, CredentialStore},
    ConnectionError,
};
use futures_util::future::{AbortHandle, Abortable};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use url::Url;
use zeroize::Zeroizing;

const MAX_SCOPES: usize = 1024;

fn read_scope_index(
    store: &dyn CredentialStore,
    index: &CredentialScope,
) -> Result<Option<BTreeSet<String>>, ConnectionError> {
    let Some(bytes) = store.load(index)? else {
        return Ok(None);
    };
    if bytes.len() > MAX_SCOPES * 70 + 2 {
        return Err(ConnectionError::Storage);
    }
    let scopes: BTreeSet<String> =
        serde_json::from_slice(&bytes).map_err(|_| ConnectionError::Storage)?;
    if scopes.len() > MAX_SCOPES {
        return Err(ConnectionError::Storage);
    }
    for key in &scopes {
        CredentialScope::from_owned_key(key.clone())?;
    }
    Ok(Some(scopes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginStatus {
    SignedOut,
    SigningIn,
    SignedIn,
    RecoveryRequired,
}

struct State {
    generation: u64,
    scopes: BTreeSet<String>,
    status: LoginStatus,
    recovery_required: bool,
    abort: Option<AbortHandle>,
}
struct Shared {
    store: Arc<dyn CredentialStore>,
    index: CredentialScope,
    state: Mutex<State>,
}

/// Owns one cloud session and the credentials created through its leases.
#[derive(Clone)]
pub struct CloudSessionController {
    config: Arc<CloudConfig>,
    shared: Arc<Shared>,
}

/// A generation-bound store for cloud and relay calls. Direct environment scopes are rejected.
#[derive(Clone)]
pub struct SessionLease {
    shared: Arc<Shared>,
    generation: u64,
}

impl CredentialStore for SessionLease {
    fn load(&self, scope: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ConnectionError::Storage)?;
        self.check(&state, scope)?;
        self.shared.store.load(scope)
    }
    fn save(&self, scope: &CredentialScope, secret: &[u8]) -> Result<(), ConnectionError> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| ConnectionError::Storage)?;
        self.check(&state, scope)?;
        if !state.scopes.contains(scope.key()) {
            if state.scopes.len() >= MAX_SCOPES {
                return Err(ConnectionError::Storage);
            }
            let mut next = state.scopes.clone();
            next.insert(scope.key().to_owned());
            let index = serde_json::to_vec(&next).map_err(|_| ConnectionError::Storage)?;
            self.shared.store.save(&self.shared.index, &index)?;
            state.scopes = next;
        }
        self.shared.store.save(scope, secret)
    }
    fn delete(&self, scope: &CredentialScope) -> Result<(), ConnectionError> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ConnectionError::Storage)?;
        self.check(&state, scope)?;
        self.shared.store.delete(scope)
    }
}
impl SessionLease {
    fn check(&self, state: &State, scope: &CredentialScope) -> Result<(), ConnectionError> {
        if state.generation != self.generation || state.recovery_required || !scope.is_cloud_owned()
        {
            return Err(ConnectionError::Storage);
        }
        Ok(())
    }
}

impl CloudSessionController {
    pub fn new(
        config: CloudConfig,
        store: Arc<dyn CredentialStore>,
    ) -> Result<Self, ConnectionError> {
        let index = config.index_scope();
        let (scopes, recovery_required) = match read_scope_index(store.as_ref(), &index) {
            Ok(scopes) => (scopes.unwrap_or_default(), false),
            Err(_) => (BTreeSet::new(), true),
        };
        let shared = Arc::new(Shared {
            store,
            index,
            state: Mutex::new(State {
                generation: 0,
                scopes,
                status: if recovery_required {
                    LoginStatus::RecoveryRequired
                } else {
                    LoginStatus::SignedOut
                },
                recovery_required,
                abort: None,
            }),
        });
        let controller = Self {
            config: Arc::new(config),
            shared,
        };
        if !recovery_required && controller.load()?.is_some() {
            controller
                .shared
                .state
                .lock()
                .map_err(|_| ConnectionError::Storage)?
                .status = LoginStatus::SignedIn;
        }
        Ok(controller)
    }
    pub fn status(&self) -> Result<LoginStatus, ConnectionError> {
        Ok(self
            .shared
            .state
            .lock()
            .map_err(|_| ConnectionError::Storage)?
            .status)
    }
    pub fn lease(&self) -> Result<SessionLease, ConnectionError> {
        let generation = self
            .shared
            .state
            .lock()
            .map_err(|_| ConnectionError::Storage)?
            .generation;
        Ok(SessionLease {
            shared: self.shared.clone(),
            generation,
        })
    }
    pub fn load(&self) -> Result<Option<CloudGrant>, ConnectionError> {
        self.config.load(&self.lease()?)
    }
    /// Load a grant together with the lease that must accompany its relay work.
    pub fn load_session(&self) -> Result<Option<(CloudGrant, SessionLease)>, ConnectionError> {
        let lease = self.lease()?;
        Ok(self.config.load(&lease)?.map(|grant| (grant, lease)))
    }
    pub fn config(&self) -> &CloudConfig {
        &self.config
    }

    pub async fn begin_login(&self) -> Result<(Url, SessionPendingLogin), ConnectionError> {
        let (generation, registration) = {
            let mut state = self
                .shared
                .state
                .lock()
                .map_err(|_| ConnectionError::Storage)?;
            if state.status != LoginStatus::SignedOut {
                return Err(ConnectionError::PermissionDenied);
            }
            let (abort, registration) = AbortHandle::new_pair();
            state.abort = Some(abort);
            state.status = LoginStatus::SigningIn;
            (state.generation, registration)
        };
        let guard = LoginGuard {
            shared: self.shared.clone(),
            generation,
        };
        let (url, pending) = self.config.begin_login().await?;
        {
            let state = self
                .shared
                .state
                .lock()
                .map_err(|_| ConnectionError::Storage)?;
            if state.generation != generation || state.status != LoginStatus::SigningIn {
                return Err(ConnectionError::PermissionDenied);
            }
        }
        Ok((
            url,
            SessionPendingLogin {
                config: self.config.clone(),
                pending,
                lease: SessionLease {
                    shared: self.shared.clone(),
                    generation,
                },
                registration,
                guard,
            },
        ))
    }
    pub fn cancel_login(&self) -> Result<(), ConnectionError> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| ConnectionError::Storage)?;
        if state.status == LoginStatus::SigningIn {
            state.generation = state
                .generation
                .checked_add(1)
                .ok_or(ConnectionError::Storage)?;
            if let Some(abort) = state.abort.take() {
                abort.abort();
            }
            state.status = LoginStatus::SignedOut;
        }
        Ok(())
    }
    pub fn disconnect(&self) -> Result<(), ConnectionError> {
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| ConnectionError::Storage)?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or(ConnectionError::Storage)?;
        if let Some(abort) = state.abort.take() {
            abort.abort();
        }
        if state.recovery_required {
            let scopes = read_scope_index(self.shared.store.as_ref(), &self.shared.index)?
                .ok_or(ConnectionError::Storage)?;
            state.scopes.extend(scopes);
            state.recovery_required = false;
        }
        let mut failed = false;
        for scope in self.config.owned_scopes() {
            if self.shared.store.delete(&scope).is_err() {
                failed = true;
            }
        }
        for key in &state.scopes {
            let scope = CredentialScope::from_owned_key(key.clone())?;
            if self.shared.store.delete(&scope).is_err() {
                failed = true;
            }
        }
        if failed || state.recovery_required {
            return Err(ConnectionError::Storage);
        }
        self.shared.store.delete(&self.shared.index)?;
        state.scopes.clear();
        state.status = LoginStatus::SignedOut;
        Ok(())
    }
}

struct LoginGuard {
    shared: Arc<Shared>,
    generation: u64,
}
impl Drop for LoginGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            if state.generation == self.generation && state.status == LoginStatus::SigningIn {
                if let Some(next) = state.generation.checked_add(1) {
                    state.generation = next;
                }
                state.abort.take();
                state.status = LoginStatus::SignedOut;
            }
        }
    }
}
/// Single-use login future handle. Dropping it cancels the login and invalidates its lease.
pub struct SessionPendingLogin {
    config: Arc<CloudConfig>,
    pending: PendingLogin,
    lease: SessionLease,
    registration: futures_util::future::AbortRegistration,
    guard: LoginGuard,
}
impl SessionPendingLogin {
    pub async fn complete(self) -> Result<(CloudGrant, SessionLease), ConnectionError> {
        let Self {
            config,
            pending,
            lease,
            registration,
            guard,
        } = self;
        let result = Abortable::new(pending.complete(&config, &lease), registration)
            .await
            .map_err(|_| ConnectionError::PermissionDenied)?;
        if result.is_ok() {
            let mut state = guard
                .shared
                .state
                .lock()
                .map_err(|_| ConnectionError::Storage)?;
            if state.generation != guard.generation {
                return Err(ConnectionError::PermissionDenied);
            }
            state.abort = None;
            state.status = LoginStatus::SignedIn;
        }
        result.map(|grant| (grant, lease))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{CloudPurpose, MemoryCredentialStore};
    use bar_identity::EnvironmentId;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn callback(url: &Url) {
        let params: std::collections::HashMap<_, _> =
            url::form_urlencoded::parse(url.fragment().unwrap().as_bytes())
                .into_owned()
                .collect();
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", params["port"]))
            .await
            .unwrap();
        stream
            .write_all(
                format!(
                    "GET /callback?state={}&code=fixture HTTP/1.1\r\nHost: localhost\r\n\r\n",
                    params["state"]
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200"));
    }
    fn server() -> (
        String,
        mpsc::Receiver<()>,
        mpsc::Sender<()>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let (seen_tx, seen_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut b = [0];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut b).unwrap();
                head.push(b[0]);
            }
            let head = String::from_utf8(head).unwrap();
            let len: usize = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|v| v.parse().ok())
                })
                .unwrap();
            let mut body = vec![0; len];
            stream.read_exact(&mut body).unwrap();
            seen_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            let body = r#"{"access_token":"synthetic","refresh_token":"refresh","token_type":"Bearer","expires_in":3600}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        });
        (origin, seen_rx, release_tx, handle)
    }
    #[test]
    fn keychain_read_failure_is_not_a_signed_out_session() {
        struct DeniedGrantStore;
        impl CredentialStore for DeniedGrantStore {
            fn load(&self, scope: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> {
                if scope.is_cloud_owned() { Err(ConnectionError::Storage) } else { Ok(None) }
            }
            fn save(&self, _: &CredentialScope, _: &[u8]) -> Result<(), ConnectionError> { panic!("startup must not save") }
            fn delete(&self, _: &CredentialScope) -> Result<(), ConnectionError> { panic!("startup must not delete") }
        }
        let controller = CloudSessionController::new(
            CloudConfig::production().unwrap(), Arc::new(DeniedGrantStore),
        );
        assert!(matches!(controller, Err(ConnectionError::Storage)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn login_restart_duplicate_cancel_and_disconnect_during_exchange() {
        let (origin, seen, release, server_thread) = server();
        let store = Arc::new(MemoryCredentialStore::default());
        let config = CloudConfig::loopback_fixture(&origin, &origin).unwrap();
        let controller = CloudSessionController::new(config.clone(), store.clone()).unwrap();
        let (url, pending) = controller.begin_login().await.unwrap();
        assert_eq!(controller.status().unwrap(), LoginStatus::SigningIn);
        assert!(controller.begin_login().await.is_err());
        let task = tokio::spawn(pending.complete());
        callback(&url).await;
        seen.recv().unwrap();
        release.send(()).unwrap();
        let (grant, _) = task.await.unwrap().unwrap();
        assert_eq!(controller.status().unwrap(), LoginStatus::SignedIn);
        drop(controller);
        let restarted = CloudSessionController::new(config.clone(), store.clone()).unwrap();
        assert_eq!(restarted.status().unwrap(), LoginStatus::SignedIn);
        assert_eq!(
            restarted.load().unwrap().unwrap().cache_key(),
            grant.cache_key()
        );
        restarted.disconnect().unwrap();
        assert!(restarted.load().unwrap().is_none());
        server_thread.join().unwrap();

        let (origin, seen, release, server_thread) = server();
        let config = CloudConfig::loopback_fixture(&origin, &origin).unwrap();
        let controller = CloudSessionController::new(config.clone(), store.clone()).unwrap();
        let (_, pending) = controller.begin_login().await.unwrap();
        controller.cancel_login().unwrap();
        assert!(pending.complete().await.is_err());
        let (url, pending) = controller.begin_login().await.unwrap();
        let task = tokio::spawn(pending.complete());
        callback(&url).await;
        seen.recv().unwrap();
        controller.disconnect().unwrap();
        release.send(()).unwrap();
        assert!(task.await.unwrap().is_err());
        assert!(controller.load().unwrap().is_none());
        server_thread.join().unwrap();
    }

    struct DelayedStore {
        inner: MemoryCredentialStore,
        target: CredentialScope,
        entered: Mutex<Option<mpsc::Sender<()>>>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    struct UnreadableIndexStore {
        inner: Arc<MemoryCredentialStore>,
        index: CredentialScope,
        unreadable: std::sync::atomic::AtomicBool,
    }
    impl CredentialStore for UnreadableIndexStore {
        fn load(
            &self,
            scope: &CredentialScope,
        ) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> {
            if *scope == self.index && self.unreadable.load(std::sync::atomic::Ordering::Acquire) {
                return Err(ConnectionError::Storage);
            }
            self.inner.load(scope)
        }
        fn save(&self, scope: &CredentialScope, bytes: &[u8]) -> Result<(), ConnectionError> {
            self.inner.save(scope, bytes)
        }
        fn delete(&self, scope: &CredentialScope) -> Result<(), ConnectionError> {
            self.inner.delete(scope)
        }
    }
    #[test]
    fn unreadable_scope_index_cannot_be_replaced_with_an_empty_index() {
        let config =
            CloudConfig::loopback_fixture("http://127.0.0.1/", "http://127.0.0.1/").unwrap();
        let index = config.index_scope();
        let relay = CredentialScope::relay_access(
            "http://127.0.0.1/",
            &EnvironmentId::new("fixture").unwrap(),
            "principal",
            "key",
        );
        let store = Arc::new(UnreadableIndexStore {
            inner: Arc::new(MemoryCredentialStore::default()),
            index: index.clone(),
            unreadable: std::sync::atomic::AtomicBool::new(true),
        });
        let bytes = serde_json::to_vec(&BTreeSet::from([relay.key().to_owned()])).unwrap();
        store.save(&index, &bytes).unwrap();
        store.save(&relay, b"synthetic-relay").unwrap();
        let recovered = CloudSessionController::new(config.clone(), store.clone()).unwrap();
        assert_eq!(recovered.status().unwrap(), LoginStatus::RecoveryRequired);
        assert!(recovered.disconnect().is_err());
        assert_eq!(store.inner.load(&index).unwrap().unwrap().as_slice(), bytes);
        store
            .unreadable
            .store(false, std::sync::atomic::Ordering::Release);
        recovered.disconnect().unwrap();
        assert_eq!(recovered.status().unwrap(), LoginStatus::SignedOut);
        let recovered = CloudSessionController::new(config, store.inner.clone()).unwrap();
        recovered.disconnect().unwrap();
        assert!(store.inner.load(&relay).unwrap().is_none());
        assert!(store.inner.load(&index).unwrap().is_none());
    }
    impl CredentialStore for DelayedStore {
        fn load(&self, s: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> {
            self.inner.load(s)
        }
        fn save(&self, s: &CredentialScope, bytes: &[u8]) -> Result<(), ConnectionError> {
            if *s == self.target {
                self.entered
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap()
                    .send(())
                    .unwrap();
                self.release.lock().unwrap().recv().unwrap();
            }
            self.inner.save(s, bytes)
        }
        fn delete(&self, s: &CredentialScope) -> Result<(), ConnectionError> {
            self.inner.delete(s)
        }
    }
    #[test]
    fn disconnect_orders_inflight_relay_save_and_preserves_unrelated_scopes() {
        let config =
            CloudConfig::loopback_fixture("http://127.0.0.1/", "http://127.0.0.1/").unwrap();
        let relay = CredentialScope::relay_access(
            "http://127.0.0.1/",
            &EnvironmentId::new("fixture").unwrap(),
            "principal",
            "key",
        );
        let unrelated = CredentialScope::cloud("https://other.example/", CloudPurpose::Grant);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let store = Arc::new(DelayedStore {
            inner: MemoryCredentialStore::default(),
            target: relay.clone(),
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(release_rx),
        });
        store.save(&unrelated, b"keep").unwrap();
        let controller = CloudSessionController::new(config, store.clone()).unwrap();
        let lease = controller.lease().unwrap();
        let old = lease.clone();
        let scope = relay.clone();
        let writer = thread::spawn(move || lease.save(&scope, b"relay"));
        entered_rx.recv().unwrap();
        let c = controller.clone();
        let disconnect = thread::spawn(move || c.disconnect());
        release_tx.send(()).unwrap();
        writer.join().unwrap().unwrap();
        disconnect.join().unwrap().unwrap();
        assert!(store.load(&relay).unwrap().is_none());
        assert!(old.save(&relay, b"late").is_err());
        assert_eq!(store.load(&unrelated).unwrap().unwrap().as_slice(), b"keep");
    }

    #[test]
    fn corrupt_scope_index_enters_recovery_and_disconnect_cannot_claim_success() {
        let config =
            CloudConfig::loopback_fixture("http://127.0.0.1/", "http://127.0.0.1/").unwrap();
        let store = Arc::new(MemoryCredentialStore::default());
        store
            .save(&config.index_scope(), b"not-a-scope-index")
            .unwrap();
        let controller = CloudSessionController::new(config, store.clone()).unwrap();
        assert_eq!(controller.status().unwrap(), LoginStatus::RecoveryRequired);
        assert!(controller.load().is_err());
        assert!(controller.disconnect().is_err());
        assert!(store.load(&controller.shared.index).unwrap().is_some());
    }
}
