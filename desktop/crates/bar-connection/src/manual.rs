//! Bar-owned connections. Desktop catalog entries and credentials are never modified.
use crate::{
    catalog::{EnvironmentMetadata, Route, SshTarget},
    credentials::CredentialStore,
    ssh::{self, SshConnectOptions, SshTunnel},
    ConnectionError, Grant, VerifiedEndpoint,
};
use bar_identity::EnvironmentId;
use serde::{Deserialize, Serialize};
use std::{fs, io, io::Write, path::PathBuf};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const MAX_ENVIRONMENTS: usize = 64;

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ConnectionInput {
    Direct {
        address: String,
    },
    Ssh {
        hostname: String,
        username: Option<String>,
        port: Option<u16>,
    },
}

impl Drop for ConnectionInput {
    fn drop(&mut self) {
        if let Self::Direct { address } = self {
            address.zeroize();
        }
    }
}

pub struct PairingKey(Zeroizing<String>);
impl PairingKey {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn input_has_pairing_key(input: &ConnectionInput) -> Result<bool, String> {
    Ok(parse_input(input)?.code.is_some())
}

struct ParsedInput {
    route: Route,
    code: Option<Zeroizing<String>>,
}

fn invalid_input() -> String {
    "Enter an HTTPS server address, a pairing link, or a valid SSH host. HTTP requires loopback or a verified Tailscale peer.".into()
}

fn parse_input(input: &ConnectionInput) -> Result<ParsedInput, String> {
    match input {
        ConnectionInput::Ssh {
            hostname,
            username,
            port,
        } => {
            let target = SshTarget {
                alias: hostname.trim().into(),
                hostname: hostname.trim().into(),
                username: username
                    .as_ref()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim().into()),
                port: *port,
            };
            ssh::credential_identity(&target).map_err(|_| invalid_input())?;
            Ok(ParsedInput {
                route: Route::Ssh {
                    connection_id: "bar-manual".into(),
                    target: Some(target),
                },
                code: None,
            })
        }
        ConnectionInput::Direct { address } => {
            if address.len() > 8192 {
                return Err(invalid_input());
            }
            let mut address = Zeroizing::new(address.trim().to_string());
            let mut url = Url::parse(&address).map_err(|_| invalid_input())?;
            if url.scheme() == "t3code" {
                if url.host_str() != Some("pair") {
                    return Err(invalid_input());
                }
                let links: Vec<_> = url
                    .query_pairs()
                    .filter(|(k, _)| k == "pairingUrl")
                    .collect();
                if links.len() != 1 {
                    return Err(invalid_input());
                }
                address = Zeroizing::new(links[0].1.to_string());
                url = Url::parse(&address).map_err(|_| invalid_input())?;
            }
            if url.scheme() == "https"
                && url.host_str() == Some("app.t3.codes")
                && url.path() == "/pair"
            {
                let hosts: Vec<_> = url.query_pairs().filter(|(k, _)| k == "host").collect();
                if hosts.len() != 1 {
                    return Err(invalid_input());
                }
                let host = hosts[0].1.to_string();
                let mut base = Url::parse(&host)
                    .or_else(|_| Url::parse(&format!("https://{host}")))
                    .map_err(|_| invalid_input())?;
                base.set_fragment(url.fragment());
                url = base;
            }
            let code = if let Some(fragment) = url.fragment() {
                let fields: Vec<_> = url::form_urlencoded::parse(fragment.as_bytes()).collect();
                if fields.len() != 1 || fields[0].0 != "token" {
                    return Err(invalid_input());
                }
                Some(Zeroizing::new(fields[0].1.to_string()))
            } else {
                None
            };
            if code.is_some() && url.path() == "/pair" {
                url.set_path("/");
            }
            url.set_fragment(None);
            let base = crate::normalized_origin(url.as_str()).map_err(|_| invalid_input())?;
            Ok(ParsedInput {
                route: Route::Bearer {
                    connection_id: "bar-manual".into(),
                    http_base_url: Some(base.to_string()),
                },
                code,
            })
        }
    }
}

pub fn input_route(input: &ConnectionInput) -> Result<Route, String> {
    Ok(parse_input(input)?.route)
}

pub fn destination_label(route: &Route) -> String {
    match route {
        Route::Bearer { http_base_url: Some(url), .. } => url.clone(),
        Route::Ssh { target: Some(target), .. } => {
            let user = target.username.as_ref().map(|user| format!("{user}@")).unwrap_or_default();
            let port = target.port.map(|port| format!(":{port}")).unwrap_or_default();
            format!("{user}{}{port}", target.hostname)
        }
        _ => String::new(),
    }
}

pub fn same_destination(a: &Route, b: &Route) -> bool {
    match (a, b) {
        (
            Route::Bearer {
                http_base_url: Some(a),
                ..
            }
            | Route::Primary { http_base_url: a },
            Route::Bearer {
                http_base_url: Some(b),
                ..
            }
            | Route::Primary { http_base_url: b },
        ) => crate::normalized_origin(a)
            .ok()
            .zip(crate::normalized_origin(b).ok())
            .is_some_and(|(a, b)| a == b),
        (
            Route::Ssh {
                target: Some(a), ..
            },
            Route::Ssh {
                target: Some(b), ..
            },
        ) => ssh::credential_identity(a)
            .ok()
            .zip(ssh::credential_identity(b).ok())
            .is_some_and(|(a, b)| a == b),
        _ => false,
    }
}

pub fn connection_error(error: ConnectionError) -> String {
    match error {
        ConnectionError::WrongEnvironment => "This address now belongs to a different environment. No pairing key was sent.".into(),
        ConnectionError::PairingRejected | ConnectionError::PermissionDenied => "The pairing key was not accepted. It may be expired, already used, or revoked. Create a new key in the destination environment.".into(),
        ConnectionError::InvalidRoute => invalid_input(),
        ConnectionError::Storage => "Could not save access to the system credential store. Existing access was kept where possible.".into(),
        ConnectionError::Transport => "Could not reach the server. Check the address and whether T3 Code is running.".into(),
        _ => "This server did not provide a supported T3 Code connection.".into(),
    }
}

async fn open(
    input: &ConnectionInput,
    options: &SshConnectOptions,
) -> Result<(ParsedInput, VerifiedEndpoint, Option<SshTunnel>), String> {
    let parsed = parse_input(input)?;
    match &parsed.route {
        Route::Bearer {
            http_base_url: Some(url),
            ..
        } => {
            let endpoint = VerifiedEndpoint::identify(url)
                .await
                .map_err(connection_error)?;
            Ok((parsed, endpoint, None))
        }
        Route::Ssh {
            target: Some(target),
            ..
        } => {
            let tunnel = ssh::connect(target, options).map_err(|_| "Could not connect over SSH. Check the host, SSH agent, known host key, and running T3 Code server.".to_string())?;
            let endpoint = tunnel.identify_endpoint().await.map_err(connection_error)?;
            Ok((parsed, endpoint, Some(tunnel)))
        }
        _ => Err(invalid_input()),
    }
}

pub async fn probe(
    input: &ConnectionInput,
    options: &SshConnectOptions,
) -> Result<EnvironmentMetadata, String> {
    let (parsed, endpoint, _tunnel) = open(input, options).await?;
    Ok(EnvironmentMetadata {
        id: endpoint.descriptor().id.clone(),
        label: endpoint.descriptor().label.clone(),
        enabled: true,
        routes: vec![parsed.route],
    })
}

pub struct PreparedConnection {
    metadata: EnvironmentMetadata,
    endpoint: VerifiedEndpoint,
    grant: Grant,
    _tunnel: Option<SshTunnel>,
}

impl PreparedConnection {
    pub async fn pair(
        input: &ConnectionInput,
        expected: &EnvironmentId,
        code: &str,
        options: &SshConnectOptions,
    ) -> Result<Self, String> {
        let (parsed, endpoint, tunnel) = open(input, options).await?;
        if endpoint.descriptor().id != *expected {
            return Err(connection_error(ConnectionError::WrongEnvironment));
        }
        if endpoint.descriptor().label.trim().is_empty() || endpoint.descriptor().label.len() > 256
        {
            return Err("The server provided an invalid environment name.".into());
        }
        if let Some(embedded) = parsed.code.as_ref() {
            if !code.trim().is_empty() && !embedded.trim().eq_ignore_ascii_case(code.trim()) {
                return Err("The entered pairing key differs from the key in the link.".into());
            }
        }
        let code = if code.trim().is_empty() {
            parsed.code.as_deref().map(|s| s.as_str()).unwrap_or("")
        } else {
            code
        };
        let grant = endpoint.pair(code).await.map_err(connection_error)?;
        let metadata = EnvironmentMetadata {
            id: expected.clone(),
            label: endpoint.descriptor().label.clone(),
            enabled: true,
            routes: vec![parsed.route],
        };
        Ok(Self {
            metadata,
            endpoint,
            grant,
            _tunnel: tunnel,
        })
    }

    /// Publish metadata only after its own grant is saved. Restore the old grant
    /// if metadata cannot be committed, rather than replacing working access.
    pub fn commit(
        self,
        registry: &mut ManualRegistry,
        store: &dyn CredentialStore,
    ) -> Result<(), String> {
        let scope = self.endpoint.credential_scope();
        let previous = store.load(&scope).map_err(connection_error)?;
        let next = registry.with_entry(self.metadata)?;
        self.endpoint
            .save_grant(&self.grant, store)
            .map_err(connection_error)?;
        if let Err(error) = registry.persist(&next) {
            let restored = match previous {
                Some(bytes) => store.save(&scope, &bytes),
                None => store.delete(&scope),
            };
            if restored.is_err() {
                return Err("Connection settings could not be saved and credential restoration failed. Check the system credential store before retrying.".into());
            }
            return Err(format!("Connection settings could not be saved: {error}"));
        }
        registry.entries = next;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    environments: Vec<EnvironmentMetadata>,
}

#[derive(Clone)]
pub struct ManualRegistry {
    path: PathBuf,
    entries: Vec<EnvironmentMetadata>,
    writable: bool,
}

impl ManualRegistry {
    pub fn unavailable(path: PathBuf) -> Self {
        Self {
            path,
            entries: vec![],
            writable: false,
        }
    }
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let mut registry = Self {
            path,
            entries: vec![],
            writable: true,
        };
        let bytes = match fs::read(&registry.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(registry),
            Err(error) => return Err(error),
        };
        if bytes.len() > 1024 * 1024 {
            return Err(io::Error::other("Connection file is too large"));
        }
        let document: Document = serde_json::from_slice(&bytes)?;
        if document.version != 1 || document.environments.len() > MAX_ENVIRONMENTS {
            return Err(io::Error::other("Unsupported connection settings"));
        }
        for entry in document.environments {
            registry.entries = registry.with_entry(entry).map_err(io::Error::other)?;
        }
        Ok(registry)
    }
    pub fn entries(&self) -> &[EnvironmentMetadata] {
        &self.entries
    }
    fn with_entry(&self, entry: EnvironmentMetadata) -> Result<Vec<EnvironmentMetadata>, String> {
        if entry.label.trim().is_empty()
            || entry.label.len() > 256
            || entry.routes.is_empty()
            || entry.routes.len() > 16
        {
            return Err("Invalid saved environment".into());
        }
        for route in &entry.routes {
            match route {
                Route::Bearer {
                    http_base_url: Some(url),
                    ..
                } => {
                    crate::normalized_origin(url).map_err(connection_error)?;
                }
                Route::Ssh {
                    target: Some(target),
                    ..
                } => {
                    ssh::credential_identity(target).map_err(|_| invalid_input())?;
                }
                _ => return Err("Only direct and SSH connections can be saved by Bar".into()),
            }
            if self.entries.iter().any(|old| {
                old.id != entry.id && old.routes.iter().any(|r| same_destination(r, route))
            }) {
                return Err(connection_error(ConnectionError::WrongEnvironment));
            }
        }
        let mut next = self.entries.clone();
        if let Some(old) = next.iter_mut().find(|old| old.id == entry.id) {
            for route in entry.routes {
                if !old.routes.iter().any(|r| same_destination(r, &route)) {
                    old.routes.push(route);
                }
            }
            if old.routes.len() > 16 {
                return Err("Too many saved routes for this environment".into());
            }
            old.label = entry.label;
        } else {
            if next.len() >= MAX_ENVIRONMENTS {
                return Err("Too many saved environments".into());
            }
            next.push(entry);
        }
        Ok(next)
    }
    pub fn remove(&mut self, id: &EnvironmentId) -> io::Result<()> {
        let next: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.id != *id)
            .cloned()
            .collect();
        self.persist(&next)?;
        self.entries = next;
        Ok(())
    }
    pub fn merge(&self, discovered: Vec<EnvironmentMetadata>) -> Vec<EnvironmentMetadata> {
        let mut entries = self.entries.clone();
        for discovered in discovered {
            if let Some(entry) = entries.iter_mut().find(|entry| entry.id == discovered.id) {
                entry.enabled = discovered.enabled;
                for route in discovered.routes {
                    if !entry
                        .routes
                        .iter()
                        .any(|r| same_destination(r, &route) || r == &route)
                    {
                        entry.routes.push(route);
                    }
                }
            } else {
                entries.push(discovered);
            }
        }
        entries
    }
    fn persist(&self, entries: &[EnvironmentMetadata]) -> io::Result<()> {
        if !self.writable {
            return Err(io::Error::other("Saved connections could not be loaded; the original file is preserved. Repair it and restart before saving."));
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| {
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(&Document {
                version: 1,
                environments: entries.to_vec(),
            })?)?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryCredentialStore;
    use std::{
        io::Read,
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread,
        time::Duration,
    };

    struct FixtureServer {
        address: String,
        stop: Arc<AtomicBool>,
        pairs: Arc<AtomicUsize>,
        thread: Option<thread::JoinHandle<()>>,
    }
    impl FixtureServer {
        fn new() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = format!("http://{}/", listener.local_addr().unwrap());
            let stop = Arc::new(AtomicBool::new(false));
            let pairs = Arc::new(AtomicUsize::new(0));
            let (done, requests) = (stop.clone(), pairs.clone());
            let thread = thread::spawn(move || {
                while !done.load(Ordering::Acquire) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(stream) => stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                            continue;
                        }
                        Err(error) => panic!("{error}"),
                    };
                    // Accepted sockets inherit O_NONBLOCK on macOS/BSD.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut byte = [0];
                    while !bytes.ends_with(b"\r\n\r\n") {
                        stream.read_exact(&mut byte).unwrap();
                        bytes.push(byte[0]);
                    }
                    let head = String::from_utf8(bytes).unwrap();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|v| v.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).unwrap();
                    let (status, response) = if head.starts_with("GET /.well-known/t3/environment ")
                    {
                        assert!(!head.to_ascii_lowercase().contains("authorization:"));
                        (
                            "200 OK",
                            r#"{"environmentId":"fixture-manual","label":"Independent server","platform":{"os":"linux","arch":"x64"},"serverVersion":"test","orchestrationProtocolVersion":2,"capabilities":{}}"#,
                        )
                    } else {
                        assert!(head.starts_with("POST /oauth/token "));
                        requests.fetch_add(1, Ordering::AcqRel);
                        let body = String::from_utf8(body).unwrap();
                        assert!(body.contains("scope=orchestration%3Aread+diagnostics%3Aread"));
                        if body.contains("subject_token=ABCD2345EFGH") {
                            (
                                "200 OK",
                                r#"{"access_token":"synthetic-manual-grant","token_type":"Bearer","expires_in":3600,"scope":"orchestration:read diagnostics:read"}"#,
                            )
                        } else {
                            ("401 Unauthorized", "{}")
                        }
                    };
                    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
                }
            });
            Self {
                address,
                stop,
                pairs,
                thread: Some(thread),
            }
        }
        fn input(&self) -> ConnectionInput {
            ConnectionInput::Direct {
                address: self.address.clone(),
            }
        }
    }
    impl Drop for FixtureServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            self.thread.take().unwrap().join().unwrap();
        }
    }
    #[test]
    fn fixture_waits_for_a_slow_client_request() {
        let server = FixtureServer::new();
        let address = Url::parse(&server.address).unwrap();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", address.port().unwrap())).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        thread::sleep(Duration::from_millis(300));
        client.write_all(b"GET /.well-known/t3/environment HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("fixture-manual"));
    }

    struct FixtureDir(PathBuf);
    impl FixtureDir {
        fn new() -> Self {
            let id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("t3-bar-manual-test-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for FixtureDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[tokio::test]
    async fn add_without_a_catalog_restores_after_restart_and_rejected_code_keeps_access() {
        let server = FixtureServer::new();
        let dir = FixtureDir::new();
        let path = dir.0.join("connections.json");
        let mut registry = ManualRegistry::load(path.clone()).unwrap();
        let store = MemoryCredentialStore::default();
        let options = SshConnectOptions::default();
        let candidate = probe(&server.input(), &options).await.unwrap();
        PreparedConnection::pair(&server.input(), &candidate.id, "ABCD2345EFGH", &options)
            .await
            .unwrap()
            .commit(&mut registry, &store)
            .unwrap();
        let endpoint = VerifiedEndpoint::identify(&server.address).await.unwrap();
        let previous = store.load(&endpoint.credential_scope()).unwrap().unwrap();
        let file = fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&file).contains("synthetic-manual-grant"));
        assert!(!String::from_utf8_lossy(&file).contains("ABCD2345EFGH"));
        let rejected =
            PreparedConnection::pair(&server.input(), &candidate.id, "ZZZZZZZZZZZZ", &options)
                .await;
        assert!(rejected.err().unwrap().contains("not accepted"));
        assert_eq!(
            store
                .load(&endpoint.credential_scope())
                .unwrap()
                .unwrap()
                .as_slice(),
            previous.as_slice()
        );
        assert_eq!(fs::read(&path).unwrap(), file);
        let restored = ManualRegistry::load(path).unwrap();
        assert_eq!(restored.entries().len(), 1);
        let grant = endpoint.load_saved_grant(&store).unwrap().unwrap();
        assert!(grant.orchestration_read && grant.diagnostics_read);
        assert_eq!(server.pairs.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn changed_identity_never_receives_a_pairing_key_and_failed_metadata_save_restores_grant()
    {
        let server = FixtureServer::new();
        let options = SshConnectOptions::default();
        let wrong = PreparedConnection::pair(
            &server.input(),
            &EnvironmentId::new("different").unwrap(),
            "ABCD2345EFGH",
            &options,
        )
        .await;
        assert!(wrong.err().unwrap().contains("different environment"));
        assert_eq!(server.pairs.load(Ordering::Acquire), 0);
        let endpoint = VerifiedEndpoint::identify(&server.address).await.unwrap();
        let store = MemoryCredentialStore::default();
        store
            .save(&endpoint.credential_scope(), b"previous-synthetic-grant")
            .unwrap();
        let dir = FixtureDir::new();
        let path = dir.0.join("connections.json");
        fs::write(&path, b"malformed-original").unwrap();
        assert!(ManualRegistry::load(path.clone()).is_err());
        let mut registry = ManualRegistry::unavailable(path.clone());
        let prepared = PreparedConnection::pair(
            &server.input(),
            &endpoint.descriptor().id,
            "ABCD2345EFGH",
            &options,
        )
        .await
        .unwrap();
        assert!(prepared.commit(&mut registry, &store).is_err());
        assert_eq!(
            store
                .load(&endpoint.credential_scope())
                .unwrap()
                .unwrap()
                .as_slice(),
            b"previous-synthetic-grant"
        );
        assert_eq!(fs::read(path).unwrap(), b"malformed-original");
        assert!(registry.entries().is_empty());
    }

    #[test]
    fn pairing_links_are_parsed_without_persisting_their_secrets() {
        for address in [
            "https://server.example/pair#token=ABCD2345EFGH",
            "t3code://pair?pairingUrl=https%3A%2F%2Fserver.example%2Fpair%23token%3DABCD2345EFGH",
            "https://app.t3.codes/pair?host=https%3A%2F%2Fserver.example#token=ABCD2345EFGH",
        ] {
            let parsed = parse_input(&ConnectionInput::Direct {
                address: address.into(),
            })
            .unwrap();
            assert_eq!(parsed.code.as_deref().unwrap().as_str(), "ABCD2345EFGH");
            assert!(
                matches!(parsed.route, Route::Bearer { http_base_url: Some(url), .. } if url == "https://server.example/")
            );
        }
        for address in [
            "https://user:secret@server.example/",
            "https://server.example/?token=secret",
            "https://server.example/path",
            "https://server.example/pair#token=one&token=two",
            "t3code://threads/remote/thread",
        ] {
            assert!(parse_input(&ConnectionInput::Direct {
                address: address.into()
            })
            .is_err());
        }
        assert!(crate::validated_base("http://192.168.1.20:3773/").is_err());
        assert!(parse_input(&ConnectionInput::Ssh {
            hostname: "host;command".into(),
            username: None,
            port: None
        })
        .is_err());
    }

    #[test]
    fn manual_and_desktop_routes_deduplicate_and_removal_leaves_desktop_untouched() {
        let dir = FixtureDir::new();
        let mut registry = ManualRegistry::load(dir.0.join("connections.json")).unwrap();
        let route = input_route(&ConnectionInput::Direct {
            address: "https://server.example/".into(),
        })
        .unwrap();
        let entry = EnvironmentMetadata {
            id: EnvironmentId::new("fixture").unwrap(),
            label: "Fixture".into(),
            enabled: true,
            routes: vec![route],
        };
        registry.entries = registry.with_entry(entry.clone()).unwrap();
        registry.persist(&registry.entries).unwrap();
        let mut desktop = entry.clone();
        desktop.routes.push(Route::Relay);
        if let Route::Bearer {
            connection_id,
            http_base_url,
        } = &mut desktop.routes[0]
        {
            *connection_id = "desktop-route".into();
            *http_base_url = Some("https://server.example".into());
        }
        let merged = registry.merge(vec![desktop.clone()]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].routes.len(), 2);
        let mut wrong = entry.clone();
        wrong.id = EnvironmentId::new("changed-id").unwrap();
        assert!(registry.with_entry(wrong).is_err());
        registry.remove(&entry.id).unwrap();
        assert_eq!(registry.merge(vec![desktop.clone()]), vec![desktop]);
        assert!(ManualRegistry::load(dir.0.join("connections.json"))
            .unwrap()
            .entries()
            .is_empty());
    }
}
