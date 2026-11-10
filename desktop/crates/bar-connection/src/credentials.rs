use crate::{catalog::Route, ConnectionError, Grant, VerifiedEndpoint};
use bar_identity::EnvironmentId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

/// An opaque fingerprint of a grant. It identifies a session, not an account.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PrincipalIdentity([u8; 32]);

impl PrincipalIdentity {
    pub(crate) fn from_token(token: &str) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"t3-code-bar grant identity v1\0");
        hash.update(token.as_bytes());
        Self(hash.finalize().into())
    }

    /// Stable opaque key for source generation binding. It is not an account
    /// identifier and never exposes the grant secret.
    pub fn as_key(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// A key for exactly one environment and normalized trusted origin.
#[derive(Clone)]
pub struct CredentialScope(String, ScopeKind);

#[derive(Clone)]
enum ScopeKind {
    Direct,
    Cloud,
    Relay,
    Index,
}

impl PartialEq for CredentialScope {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for CredentialScope {}
impl std::hash::Hash for CredentialScope {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl CredentialScope {
    fn new(environment: &EnvironmentId, origin: &str) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"t3-code-bar credential scope v1\0");
        hash.update((environment.as_str().len() as u64).to_be_bytes());
        hash.update(environment.as_str().as_bytes());
        hash.update(origin.as_bytes());
        Self(format!("{:x}", hash.finalize()), ScopeKind::Direct)
    }
    /// Scope for an SSH target whose local tunnel origin changes per session.
    pub(crate) fn ssh(environment: &EnvironmentId, identity: &str) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"t3-code-bar ssh credential scope v1\0");
        for part in [environment.as_str(), identity] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        Self(format!("{:x}", hash.finalize()), ScopeKind::Direct)
    }
    pub(crate) fn cloud(origin: &str, purpose: CloudPurpose) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"t3-code-bar cloud credential v1\0");
        hash.update(purpose.label());
        hash.update(b"\0");
        hash.update(origin.as_bytes());
        Self(format!("{:x}", hash.finalize()), ScopeKind::Cloud)
    }
    pub(crate) fn index(origin: &str) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"t3-code-bar cloud scope index v1\0");
        hash.update(origin.as_bytes());
        Self(format!("{:x}", hash.finalize()), ScopeKind::Index)
    }
    pub(crate) fn from_owned_key(key: String) -> Result<Self, ConnectionError> {
        if key.len() != 64
            || !key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ConnectionError::Storage);
        }
        Ok(Self(key, ScopeKind::Relay))
    }
    pub(crate) fn is_cloud_owned(&self) -> bool {
        matches!(self.1, ScopeKind::Cloud | ScopeKind::Relay)
    }
    pub fn key(&self) -> &str {
        &self.0
    }
}

pub(crate) enum CloudPurpose {
    Grant,
    Signer,
}
impl CloudPurpose {
    fn label(&self) -> &'static [u8] {
        match self {
            Self::Grant => b"grant",
            Self::Signer => b"dpop-key",
        }
    }
}

impl CredentialScope {
    pub(crate) fn relay_access(
        origin: &str,
        environment: &EnvironmentId,
        principal: &str,
        thumbprint: &str,
    ) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"t3-code-bar relay access v1\0");
        for part in [origin, environment.as_str(), principal, thumbprint] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        Self(format!("{:x}", hash.finalize()), ScopeKind::Relay)
    }

    pub(crate) fn relay_environment_grant(
        origin: &str,
        environment: &EnvironmentId,
        principal: &str,
        thumbprint: &str,
    ) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"t3-code-bar relay environment grant v1\0");
        for part in [origin, environment.as_str(), principal, thumbprint] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        Self(format!("{:x}", hash.finalize()), ScopeKind::Relay)
    }
}

/// Storage adapters must protect the supplied secret and must not log it.
pub trait CredentialStore: Send + Sync {
    fn load(&self, scope: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError>;
    fn save(&self, scope: &CredentialScope, secret: &[u8]) -> Result<(), ConnectionError>;
    fn delete(&self, scope: &CredentialScope) -> Result<(), ConnectionError>;
}

/// In-memory store for synthetic fixtures. Sharing it simulates a repository restart.
#[derive(Default)]
pub struct MemoryCredentialStore(Mutex<HashMap<CredentialScope, Zeroizing<Vec<u8>>>>);
impl CredentialStore for MemoryCredentialStore {
    fn load(&self, scope: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| ConnectionError::Storage)?
            .get(scope)
            .cloned())
    }
    fn save(&self, scope: &CredentialScope, secret: &[u8]) -> Result<(), ConnectionError> {
        self.0
            .lock()
            .map_err(|_| ConnectionError::Storage)?
            .insert(scope.clone(), Zeroizing::new(secret.to_vec()));
        Ok(())
    }
    fn delete(&self, scope: &CredentialScope) -> Result<(), ConnectionError> {
        self.0
            .lock()
            .map_err(|_| ConnectionError::Storage)?
            .remove(scope);
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub struct KeyringStore {
    service: String,
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
impl KeyringStore {
    pub fn production() -> Self {
        Self::with_namespace("com.t3code.bar.connection.grants.v1")
    }
    /// Use a distinct disposable namespace for native fixture tests.
    pub fn with_namespace(namespace: impl Into<String>) -> Self {
        Self {
            service: namespace.into(),
        }
    }
    fn entry(&self, scope: &CredentialScope) -> Result<keyring::Entry, ConnectionError> {
        keyring::Entry::new(&self.service, scope.key()).map_err(|_| ConnectionError::Storage)
    }
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
impl CredentialStore for KeyringStore {
    fn load(&self, scope: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> {
        match self.entry(scope)?.get_secret() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(ConnectionError::Storage),
        }
    }
    fn save(&self, scope: &CredentialScope, secret: &[u8]) -> Result<(), ConnectionError> {
        self.entry(scope)?
            .set_secret(secret)
            .map_err(|_| ConnectionError::Storage)
    }
    fn delete(&self, scope: &CredentialScope) -> Result<(), ConnectionError> {
        match self.entry(scope)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(ConnectionError::Storage),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    environment: EnvironmentId,
    origin: String,
    token: String,
    expires_unix_ms: u64,
    orchestration_read: bool,
    diagnostics_read: bool,
    #[serde(default)]
    dpop: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dpop_thumbprint: Option<String>,
    /// Present only for transports with a stable identity (SSH). `origin` then
    /// records the session's tunnel base for diagnostics, not for trust.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    transport_identity: Option<String>,
}
impl Drop for Envelope {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.token.zeroize();
    }
}

impl VerifiedEndpoint {
    pub(crate) fn credential_scope(&self) -> CredentialScope {
        match &self.credential_identity {
            Some(identity) => CredentialScope::ssh(&self.descriptor.id, identity),
            None => CredentialScope::new(&self.descriptor.id, self.base.as_str()),
        }
    }

    /// Publish a grant only after its keyring write succeeds.
    pub async fn pair_and_save(
        &self,
        code: &str,
        store: &dyn CredentialStore,
    ) -> Result<Grant, ConnectionError> {
        let grant = self.pair(code).await?;
        self.save_grant(&grant, store)?;
        Ok(grant)
    }

    pub(crate) fn save_grant(
        &self,
        grant: &Grant,
        store: &dyn CredentialStore,
    ) -> Result<(), ConnectionError> {
        self.save_grant_at_scope(grant, store, &self.credential_scope())
    }

    pub(crate) fn save_grant_at_scope(
        &self,
        grant: &Grant,
        store: &dyn CredentialStore,
        scope: &CredentialScope,
    ) -> Result<(), ConnectionError> {
        if grant.environment != self.descriptor.id || grant.origin != self.base.as_str() {
            return Err(ConnectionError::Storage);
        }
        let millis = grant
            .expires_wall
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ConnectionError::Storage)?
            .as_millis()
            .try_into()
            .map_err(|_| ConnectionError::Storage)?;
        let envelope = Envelope {
            version: 1,
            environment: grant.environment.clone(),
            origin: grant.origin.clone(),
            token: grant.token.to_string(),
            expires_unix_ms: millis,
            orchestration_read: grant.orchestration_read,
            diagnostics_read: grant.diagnostics_read,
            dpop: grant.dpop.is_some(),
            dpop_thumbprint: grant.dpop.as_ref().map(|signer| signer.thumbprint()),
            transport_identity: self.credential_identity.clone(),
        };
        let encoded =
            Zeroizing::new(serde_json::to_vec(&envelope).map_err(|_| ConnectionError::Storage)?);
        store.save(scope, &encoded)
    }

    pub fn load_saved_grant(
        &self,
        store: &dyn CredentialStore,
    ) -> Result<Option<Grant>, ConnectionError> {
        self.load_saved_grant_inner(store, None)
            .map_err(|error| match error {
                ConnectionError::PermissionDenied => ConnectionError::GrantExpired,
                other => other,
            })
    }

    pub(crate) fn load_saved_grant_with_signer_at_scope(
        &self,
        store: &dyn CredentialStore,
        signer: &crate::dpop::DpopSigner,
        scope: &CredentialScope,
    ) -> Result<Option<Grant>, ConnectionError> {
        self.load_saved_grant_inner_at_scope(store, Some(signer), scope)
    }

    fn load_saved_grant_inner(
        &self,
        store: &dyn CredentialStore,
        signer: Option<&crate::dpop::DpopSigner>,
    ) -> Result<Option<Grant>, ConnectionError> {
        self.load_saved_grant_inner_at_scope(store, signer, &self.credential_scope())
    }

    fn load_saved_grant_inner_at_scope(
        &self,
        store: &dyn CredentialStore,
        signer: Option<&crate::dpop::DpopSigner>,
        scope: &CredentialScope,
    ) -> Result<Option<Grant>, ConnectionError> {
        let Some(bytes) = store.load(scope)? else {
            return Ok(None);
        };
        let mut envelope: Envelope =
            serde_json::from_slice(&bytes).map_err(|_| ConnectionError::Storage)?;
        if envelope.version != 1
            || envelope.environment != self.descriptor.id
            || !self.envelope_transport_matches(&envelope)
            || (!envelope.orchestration_read && !envelope.diagnostics_read)
            || envelope.token.is_empty()
            || envelope.token.len() > 16384
            || envelope.token.bytes().any(|b| b < 32 || b == 127)
            || envelope.dpop != envelope.dpop_thumbprint.is_some()
        {
            return Err(ConnectionError::Storage);
        }
        if signer.is_some() && !envelope.dpop {
            return Err(ConnectionError::Storage);
        }
        let dpop = if envelope.dpop {
            let signer = signer.ok_or(ConnectionError::Storage)?;
            if envelope.dpop_thumbprint.as_deref() != Some(signer.thumbprint().as_str()) {
                return Err(ConnectionError::Storage);
            }
            Some(signer.clone())
        } else {
            None
        };
        let expires_wall = UNIX_EPOCH
            .checked_add(Duration::from_millis(envelope.expires_unix_ms))
            .ok_or(ConnectionError::Storage)?;
        let remaining = expires_wall
            .duration_since(SystemTime::now())
            .map_err(|_| ConnectionError::PermissionDenied)?;
        if remaining.is_zero() {
            return Err(ConnectionError::PermissionDenied);
        }
        let expires_at = Instant::now()
            .checked_add(remaining)
            .ok_or(ConnectionError::Storage)?;
        Ok(Some(Grant {
            environment: envelope.environment.clone(),
            origin: self.base.as_str().to_owned(),
            token: Zeroizing::new(std::mem::take(&mut envelope.token)),
            expires_at,
            expires_wall,
            orchestration_read: envelope.orchestration_read,
            diagnostics_read: envelope.diagnostics_read,
            dpop,
        }))
    }

    fn envelope_transport_matches(&self, envelope: &Envelope) -> bool {
        match &self.credential_identity {
            None => envelope.transport_identity.is_none() && envelope.origin == self.base.as_str(),
            Some(identity) => envelope.transport_identity.as_deref() == Some(identity.as_str()),
        }
    }

    pub fn disconnect(&self, store: &dyn CredentialStore) -> Result<(), ConnectionError> {
        store.delete(&self.credential_scope())
    }
}

/// Remove Bar's own saved grant for a directly pairable route. The scope is
/// derived from the validated route alone, so no network access, tunnel or
/// stored credential is needed. Cloud and unrelated environment scopes are
/// never touched. Deleting an absent grant succeeds.
pub fn forget_saved_access(
    environment: &EnvironmentId,
    route: &Route,
    store: &dyn CredentialStore,
) -> Result<(), ConnectionError> {
    let scope = match route {
        Route::Primary { http_base_url }
        | Route::Bearer {
            http_base_url: Some(http_base_url),
            ..
        } => CredentialScope::new(environment, crate::validated_base(http_base_url)?.as_str()),
        Route::Ssh {
            target: Some(target),
            ..
        } => CredentialScope::ssh(
            environment,
            &crate::ssh::credential_identity(target).map_err(|_| ConnectionError::InvalidRoute)?,
        ),
        _ => return Err(ConnectionError::InvalidRoute),
    };
    store.delete(&scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EnvironmentDescriptor;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

    fn endpoint(id: &str, origin: &str) -> VerifiedEndpoint {
        VerifiedEndpoint {
            client: reqwest::Client::new(),
            base: crate::validated_base(origin).unwrap(),
            ws_base: url::Url::parse(origin).unwrap(),
            descriptor: EnvironmentDescriptor {
                id: EnvironmentId::new(id).unwrap(),
                label: "fixture".into(),
                machine: None,
                server_version: "1".into(),
            },
            credential_identity: None,
        }
    }
    fn envelope(id: &str, origin: &str, expires_unix_ms: u64) -> Vec<u8> {
        serde_json::to_vec(&Envelope {
            version: 1,
            environment: EnvironmentId::new(id).unwrap(),
            origin: origin.into(),
            token: "fixture-secret".into(),
            expires_unix_ms,
            orchestration_read: true,
            diagnostics_read: false,
            dpop: false,
            dpop_thumbprint: None,
            transport_identity: None,
        })
        .unwrap()
    }
    fn future() -> u64 {
        (SystemTime::now().duration_since(UNIX_EPOCH).unwrap() + Duration::from_secs(60))
            .as_millis() as u64
    }

    #[test]
    fn isolated_scopes_disconnect_and_reject_bad_envelopes() {
        let store = MemoryCredentialStore::default();
        let a = endpoint("env-a", "http://127.0.0.1:1234/");
        let b = endpoint("env-b", "http://127.0.0.1:1234/");
        let c = endpoint("env-a", "http://127.0.0.1:1235/");
        let scope = a.credential_scope();
        store
            .save(&scope, &envelope("env-a", a.base.as_str(), future()))
            .unwrap();
        assert!(b.load_saved_grant(&store).unwrap().is_none());
        assert!(c.load_saved_grant(&store).unwrap().is_none());
        store
            .save(
                &b.credential_scope(),
                &envelope("env-b", b.base.as_str(), future()),
            )
            .unwrap();
        a.disconnect(&store).unwrap();
        a.disconnect(&store).unwrap();
        assert!(a.load_saved_grant(&store).unwrap().is_none());
        assert!(b.load_saved_grant(&store).unwrap().is_some());
        for bytes in [
            b"bad".to_vec(),
            envelope("env-b", a.base.as_str(), future()),
            envelope("env-a", c.base.as_str(), future()),
            envelope("env-a", a.base.as_str(), 1),
            {
                let mut value: serde_json::Value =
                    serde_json::from_slice(&envelope("env-a", a.base.as_str(), future())).unwrap();
                value["version"] = 2.into();
                serde_json::to_vec(&value).unwrap()
            },
        ] {
            store.save(&scope, &bytes).unwrap();
            assert!(a.load_saved_grant(&store).is_err());
        }
    }
    #[test]
    fn expired_saved_grant_is_distinct_from_a_server_denial_and_is_kept() {
        let store = MemoryCredentialStore::default();
        let ep = endpoint("env-a", "http://127.0.0.1:1234/");
        store
            .save(
                &ep.credential_scope(),
                &envelope("env-a", ep.base.as_str(), 1),
            )
            .unwrap();
        assert_eq!(
            ep.load_saved_grant(&store).err(),
            Some(ConnectionError::GrantExpired)
        );
        assert!(store.load(&ep.credential_scope()).unwrap().is_some());
    }

    fn ssh_target(alias: &str) -> crate::catalog::SshTarget {
        crate::catalog::SshTarget {
            alias: alias.into(),
            hostname: "host.example".into(),
            username: None,
            port: None,
        }
    }
    fn ssh_route(alias: &str) -> Route {
        Route::Ssh {
            connection_id: "ssh-connection".into(),
            target: Some(ssh_target(alias)),
        }
    }
    fn ssh_endpoint(id: &str, alias: &str, tunnel_port: u16) -> VerifiedEndpoint {
        let mut ep = endpoint(id, &format!("http://127.0.0.1:{tunnel_port}/"));
        ep.credential_identity = Some(crate::ssh::credential_identity(&ssh_target(alias)).unwrap());
        ep
    }

    #[test]
    fn forgetting_removes_only_the_requested_environments_direct_and_ssh_grants() {
        let store = MemoryCredentialStore::default();
        let id = EnvironmentId::new("env-a").unwrap();
        let other = EnvironmentId::new("env-b").unwrap();
        let direct = endpoint("env-a", "http://127.0.0.1:4000/");
        let direct_other_origin = endpoint("env-a", "http://127.0.0.1:4001/");
        let direct_other_env = endpoint("env-b", "http://127.0.0.1:4000/");
        let ssh = ssh_endpoint("env-a", "box", 5000);
        let ssh_other_target = ssh_endpoint("env-a", "other-box", 5000);
        let ssh_other_env = ssh_endpoint("env-b", "box", 5000);
        let cloud = CredentialScope::cloud("https://cloud.example/", CloudPurpose::Grant);
        let all = [
            &direct,
            &direct_other_origin,
            &direct_other_env,
            &ssh,
            &ssh_other_target,
            &ssh_other_env,
        ];
        for ep in all {
            let mut value: serde_json::Value = serde_json::from_slice(&envelope(
                ep.descriptor.id.as_str(),
                ep.base.as_str(),
                future(),
            ))
            .unwrap();
            if let Some(identity) = &ep.credential_identity {
                value["transport_identity"] = identity.clone().into();
            }
            store
                .save(&ep.credential_scope(), &serde_json::to_vec(&value).unwrap())
                .unwrap();
        }
        store.save(&cloud, b"cloud-secret").unwrap();

        // The tunnel port is session-local and must not matter.
        let route = Route::Primary {
            http_base_url: "http://127.0.0.1:4000".into(),
        };
        forget_saved_access(&id, &route, &store).unwrap();
        forget_saved_access(&id, &route, &store).unwrap();
        forget_saved_access(&id, &ssh_route("box"), &store).unwrap();
        let loaded = |ep: &VerifiedEndpoint| ep.load_saved_grant(&store).unwrap().is_some();
        assert!(!loaded(&direct));
        assert!(!loaded(&ssh));
        assert!(!loaded(&ssh_endpoint("env-a", "box", 5999)));
        assert!(loaded(&direct_other_origin));
        assert!(loaded(&direct_other_env));
        assert!(loaded(&ssh_other_target));
        assert!(loaded(&ssh_other_env));
        assert!(store.load(&cloud).unwrap().is_some());

        let bearer = Route::Bearer {
            connection_id: "bearer".into(),
            http_base_url: Some("http://127.0.0.1:4001/".into()),
        };
        forget_saved_access(&id, &bearer, &store).unwrap();
        assert!(!loaded(&direct_other_origin));
        assert!(loaded(&direct_other_env));
        forget_saved_access(&other, &ssh_route("box"), &store).unwrap();
        assert!(!loaded(&ssh_other_env));
    }

    #[test]
    fn forgetting_rejects_routes_that_have_no_direct_grant_scope() {
        let store = MemoryCredentialStore::default();
        let id = EnvironmentId::new("env-a").unwrap();
        let invalid = [
            Route::Relay,
            Route::Bearer {
                connection_id: "b".into(),
                http_base_url: None,
            },
            Route::Primary {
                http_base_url: "http://example.com/".into(),
            },
            Route::Primary {
                http_base_url: "https://user:secret@example.com/".into(),
            },
            Route::Ssh {
                connection_id: "s".into(),
                target: None,
            },
            ssh_route("bad alias;"),
        ];
        for route in invalid {
            assert_eq!(
                forget_saved_access(&id, &route, &store),
                Err(ConnectionError::InvalidRoute),
                "{route:?}"
            );
        }
        assert_eq!(
            forget_saved_access(&id, &ssh_route("box"), &Broken),
            Err(ConnectionError::Storage)
        );
    }

    struct Broken;
    impl CredentialStore for Broken {
        fn load(&self, _: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> {
            Err(ConnectionError::Storage)
        }
        fn save(&self, _: &CredentialScope, _: &[u8]) -> Result<(), ConnectionError> {
            Err(ConnectionError::Storage)
        }
        fn delete(&self, _: &CredentialScope) -> Result<(), ConnectionError> {
            Err(ConnectionError::Storage)
        }
    }
    fn request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut b = [0];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut b).unwrap();
            bytes.push(b[0]);
        }
        let head = String::from_utf8(bytes).unwrap();
        let len = head
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|v| v.parse::<usize>().ok())
            })
            .unwrap_or(0);
        let mut body = vec![0; len];
        stream.read_exact(&mut body).unwrap();
        head + &String::from_utf8(body).unwrap()
    }
    fn reply(stream: &mut TcpStream, body: &str) {
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    }
    const DESCRIPTOR: &str = r#"{"environmentId":"env-a","label":"Test","platform":{"os":"linux","arch":"x64"},"serverVersion":"1","capabilities":{}}"#;
    #[tokio::test]
    async fn restart_load_revalidates_and_sends_only_to_bound_origin() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for step in 0..6 {
                let (mut s, _) = listener.accept().unwrap();
                let req = request(&mut s);
                match step {
                    0 | 1 | 3 | 4 => {
                        assert!(req.starts_with("GET /.well-known/t3/environment "));
                        assert!(!req.contains("fixture-secret"));
                        reply(&mut s, DESCRIPTOR);
                    }
                    2 => {
                        assert!(req.starts_with("POST /oauth/token "));
                        reply(
                            &mut s,
                            r#"{"access_token":"fixture-secret","token_type":"Bearer","expires_in":3600,"scope":"orchestration:read"}"#,
                        );
                    }
                    5 => {
                        assert!(req
                            .to_lowercase()
                            .contains("authorization: bearer fixture-secret"));
                        reply(
                            &mut s,
                            r#"{"ticket":"ticket","expiresAt":"2099-01-01T00:00:00Z"}"#,
                        );
                    }
                    _ => unreachable!(),
                }
            }
        });
        let route = format!("http://{addr}/");
        let id = EnvironmentId::new("env-a").unwrap();
        let store = MemoryCredentialStore::default();
        let first = VerifiedEndpoint::discover(&route, &id).await.unwrap();
        let grant = first.pair_and_save("ABCD2345EFGH", &store).await.unwrap();
        let identity = grant.principal_identity();
        drop(grant);
        drop(first);
        let restarted = VerifiedEndpoint::discover(&route, &id).await.unwrap();
        let loaded = restarted.load_saved_grant(&store).unwrap().unwrap();
        assert!(loaded.principal_identity() == identity);
        assert!(loaded.orchestration_read);
        assert!(!loaded.diagnostics_read);
        let other = endpoint("env-a", "http://127.0.0.1:1/");
        assert!(matches!(
            other.websocket_ticket(&loaded).await,
            Err(ConnectionError::InvalidRoute)
        ));
        assert_eq!(
            restarted.websocket_ticket(&loaded).await.unwrap().as_str(),
            "ticket"
        );
        server.join().unwrap();
    }
    #[tokio::test]
    async fn changed_descriptor_blocks_saved_secret_on_reconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for body in [
                DESCRIPTOR,
                r#"{"environmentId":"env-b","label":"Test","platform":{"os":"linux","arch":"x64"},"serverVersion":"1","capabilities":{}}"#,
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let input = request(&mut stream);
                assert!(input.starts_with("GET /.well-known/t3/environment "));
                assert!(!input.contains("fixture-secret"));
                reply(&mut stream, body);
            }
            listener.set_nonblocking(true).unwrap();
            assert!(
                matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
            );
        });
        let route = format!("http://{addr}/");
        let id = EnvironmentId::new("env-a").unwrap();
        let store = MemoryCredentialStore::default();
        let endpoint = VerifiedEndpoint::discover(&route, &id).await.unwrap();
        store
            .save(
                &endpoint.credential_scope(),
                &envelope("env-a", &route, future()),
            )
            .unwrap();
        let saved = endpoint.load_saved_grant(&store).unwrap().unwrap();
        assert!(matches!(
            endpoint.websocket_ticket(&saved).await,
            Err(ConnectionError::WrongEnvironment)
        ));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn save_failure_and_scope_widening_never_publish_grants() {
        for (scope, broken) in [
            ("orchestration:read", true),
            ("orchestration:read orchestration:write", false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let scope = scope.to_owned();
            let server = thread::spawn(move || {
                for step in 0..3 {
                    let (mut s, _) = listener.accept().unwrap();
                    let _ = request(&mut s);
                    if step < 2 {
                        reply(&mut s, DESCRIPTOR);
                    } else {
                        reply(
                            &mut s,
                            &format!(
                                r#"{{"access_token":"fixture-secret","token_type":"Bearer","expires_in":3600,"scope":"{scope}"}}"#
                            ),
                        );
                    }
                }
            });
            let ep = VerifiedEndpoint::discover(
                &format!("http://{addr}/"),
                &EnvironmentId::new("env-a").unwrap(),
            )
            .await
            .unwrap();
            let store = MemoryCredentialStore::default();
            let result = ep
                .pair_and_save("ABCD2345EFGH", if broken { &Broken } else { &store })
                .await;
            assert!(matches!(
                result,
                Err(ConnectionError::Storage | ConnectionError::Protocol)
            ));
            assert!(ep.load_saved_grant(&store).unwrap().is_none());
            server.join().unwrap();
        }
    }
}
