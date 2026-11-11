use crate::{
    credentials::{CloudPurpose, CredentialScope, CredentialStore},
    decode_response,
    dpop::DpopSigner,
    ConnectionError,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{timeout_at, Instant},
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const CLIENT_ID: &str = "hzxSgY2cH10sDU2r";
const LIFETIME: Duration = Duration::from_secs(600);

/// Trusted public OAuth configuration. HTTP is permitted only through the loopback fixture constructor.
#[derive(Clone)]
pub struct CloudConfig {
    hosted: Url,
    issuer: Url,
    client: reqwest::Client,
}
impl CloudConfig {
    pub fn production() -> Result<Self, ConnectionError> {
        Self::new("https://app.t3.codes/", "https://clerk.t3.codes/", false)
    }
    pub fn loopback_fixture(hosted: &str, issuer: &str) -> Result<Self, ConnectionError> {
        Self::new(hosted, issuer, true)
    }
    fn new(hosted: &str, issuer: &str, fixture: bool) -> Result<Self, ConnectionError> {
        fn origin(s: &str, fixture: bool) -> Result<Url, ConnectionError> {
            let u = Url::parse(s).map_err(|_| ConnectionError::InvalidRoute)?;
            let loopback = u
                .host_str()
                .and_then(|h| h.parse::<IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback());
            if !(u.scheme() == "https" && !fixture || fixture && u.scheme() == "http" && loopback)
                || u.host_str().is_none()
                || !u.username().is_empty()
                || u.password().is_some()
                || u.path() != "/"
                || u.query().is_some()
                || u.fragment().is_some()
            {
                return Err(ConnectionError::InvalidRoute);
            }
            Ok(u)
        }
        Ok(Self {
            hosted: origin(hosted, fixture)?,
            issuer: origin(issuer, fixture)?,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .user_agent("T3-Code-Bar/2.0.0-alpha.1")
                .build()
                .map_err(|_| ConnectionError::Transport)?,
        })
    }
    /// Bind the callback before returning a URL that can be opened by the user.
    pub async fn begin_login(&self) -> Result<(Url, PendingLogin), ConnectionError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| ConnectionError::Transport)?;
        let port = listener
            .local_addr()
            .map_err(|_| ConnectionError::Transport)?
            .port();
        let state = random();
        let verifier = Zeroizing::new(random());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let fragment = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", &state)
            .append_pair("challenge", &challenge)
            .append_pair("port", &port.to_string())
            .finish();
        let mut url = self
            .hosted
            .join("/connect")
            .map_err(|_| ConnectionError::InvalidRoute)?;
        url.set_fragment(Some(&fragment));
        Ok((
            url,
            PendingLogin {
                listener,
                state: Zeroizing::new(state),
                verifier,
                redirect: format!("http://127.0.0.1:{port}/callback"),
                deadline: Instant::now() + LIFETIME,
                issuer: self.issuer.clone(),
            },
        ))
    }
    pub fn signer(&self, store: &dyn CredentialStore) -> Result<DpopSigner, ConnectionError> {
        DpopSigner::load_or_create(store, self.issuer.as_str())
    }
    pub fn load(&self, store: &dyn CredentialStore) -> Result<Option<CloudGrant>, ConnectionError> {
        let Some(bytes) = store.load(&self.scope())? else {
            return Ok(None);
        };
        let mut saved: StoredGrant =
            serde_json::from_slice(&bytes).map_err(|_| ConnectionError::Storage)?;
        let grant = CloudGrant {
            version: saved.version,
            access: std::mem::take(&mut saved.access),
            refresh: std::mem::take(&mut saved.refresh),
            expires: saved.expires,
            identity: std::mem::take(&mut saved.identity),
            issuer: std::mem::take(&mut saved.issuer),
        };
        if grant.version != 1
            || grant.issuer != self.issuer.as_str()
            || !valid_secret(&grant.access)
            || !valid_secret(&grant.refresh)
            || grant.identity.len() != 43
            || grant.expires == 0
        {
            return Err(ConnectionError::Storage);
        }
        Ok(Some(grant))
    }
    pub fn disconnect(&self, store: &dyn CredentialStore) -> Result<(), ConnectionError> {
        store.delete(&self.scope())
    }
    pub(crate) fn index_scope(&self) -> CredentialScope {
        CredentialScope::index(self.issuer.as_str())
    }
    pub(crate) fn owned_scopes(&self) -> [CredentialScope; 2] {
        [
            self.scope(),
            CredentialScope::cloud(self.issuer.as_str(), CloudPurpose::Signer),
        ]
    }
    fn scope(&self) -> CredentialScope {
        CredentialScope::cloud(self.issuer.as_str(), CloudPurpose::Grant)
    }
    fn save(&self, store: &dyn CredentialStore, grant: &CloudGrant) -> Result<(), ConnectionError> {
        let saved = StoredGrant {
            version: grant.version,
            access: grant.access.clone(),
            refresh: grant.refresh.clone(),
            expires: grant.expires,
            identity: grant.identity.clone(),
            issuer: grant.issuer.clone(),
        };
        let bytes =
            Zeroizing::new(serde_json::to_vec(&saved).map_err(|_| ConnectionError::Storage)?);
        store.save(&self.scope(), &bytes)
    }
    async fn exchange(
        &self,
        form: &[(&str, &str)],
        previous: Option<&CloudGrant>,
        store: &dyn CredentialStore,
    ) -> Result<CloudGrant, ConnectionError> {
        if previous.is_some_and(|p| p.issuer != self.issuer.as_str()) {
            return Err(ConnectionError::InvalidRoute);
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ConnectionError::Protocol)?
            .as_secs();
        let response = self
            .client
            .post(self.issuer.join("/oauth/token").unwrap())
            .form(form)
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        if !response.status().is_success() {
            return Err(if response.status().is_client_error() {
                ConnectionError::PermissionDenied
            } else {
                ConnectionError::Transport
            });
        }
        let wire: Token = decode_response(response).await?;
        let access = Zeroizing::new(wire.access_token);
        let refresh = Zeroizing::new(
            wire.refresh_token
                .unwrap_or_else(|| previous.map(|p| p.refresh.clone()).unwrap_or_default()),
        );
        if !wire.token_type.eq_ignore_ascii_case("bearer")
            || !valid_secret(&access)
            || !valid_secret(&refresh)
            || !wire.expires_in.is_finite()
            || wire.expires_in < 1.0
            || wire.expires_in.fract() != 0.0
            || wire.expires_in > 315360000.0
        {
            return Err(ConnectionError::Protocol);
        }
        let grant = CloudGrant {
            version: 1,
            access: access.to_string(),
            refresh: refresh.to_string(),
            expires: now
                .checked_add(wire.expires_in as u64)
                .ok_or(ConnectionError::Protocol)?,
            identity: previous.map(|p| p.identity.clone()).unwrap_or_else(random),
            issuer: self.issuer.to_string(),
        };
        self.save(store, &grant)?;
        Ok(grant)
    }
    pub async fn refresh(
        &self,
        grant: &CloudGrant,
        store: &dyn CredentialStore,
    ) -> Result<CloudGrant, ConnectionError> {
        self.exchange(
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", &grant.refresh),
                ("client_id", CLIENT_ID),
            ],
            Some(grant),
            store,
        )
        .await
    }
}
fn random() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
fn valid_secret(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16384 && !s.bytes().any(|b| b < 33 || b == 127)
}
#[derive(Deserialize)]
struct Token {
    access_token: String,
    refresh_token: Option<String>,
    token_type: String,
    expires_in: f64,
}

/// A stored cloud session. Its identity is a random session cache key, not an account assertion.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredGrant {
    version: u32,
    access: String,
    refresh: String,
    expires: u64,
    identity: String,
    issuer: String,
}
impl Drop for StoredGrant {
    fn drop(&mut self) {
        self.access.zeroize();
        self.refresh.zeroize();
    }
}
pub struct CloudGrant {
    version: u32,
    access: String,
    refresh: String,
    expires: u64,
    identity: String,
    issuer: String,
}
impl Drop for CloudGrant {
    fn drop(&mut self) {
        self.access.zeroize();
        self.refresh.zeroize();
    }
}
impl fmt::Debug for CloudGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloudGrant")
            .field("credentials", &"[redacted]")
            .field("expires", &self.expires)
            .finish()
    }
}
impl CloudGrant {
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            version: 1,
            access: "cloud-fixture".into(),
            refresh: "refresh-fixture".into(),
            expires: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 3600,
            identity: "fixture-principal".into(),
            issuer: "http://127.0.0.1/".into(),
        }
    }
    pub fn cache_key(&self) -> &str {
        &self.identity
    }
    pub fn expires_unix_seconds(&self) -> u64 {
        self.expires
    }
    pub(crate) fn access_token(&self) -> &str {
        &self.access
    }
}

/// A single-use callback listener and its secret PKCE verifier.
pub struct PendingLogin {
    listener: TcpListener,
    state: Zeroizing<String>,
    verifier: Zeroizing<String>,
    redirect: String,
    deadline: Instant,
    issuer: Url,
}
impl fmt::Debug for PendingLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PendingLogin([redacted])")
    }
}
impl PendingLogin {
    pub async fn complete(
        self,
        config: &CloudConfig,
        store: &dyn CredentialStore,
    ) -> Result<CloudGrant, ConnectionError> {
        if self.issuer != config.issuer {
            return Err(ConnectionError::InvalidRoute);
        }
        let code = timeout_at(self.deadline, self.callback())
            .await
            .map_err(|_| ConnectionError::PermissionDenied)??;
        config
            .exchange(
                &[
                    ("grant_type", "authorization_code"),
                    ("code", &code),
                    ("redirect_uri", &self.redirect),
                    ("client_id", CLIENT_ID),
                    ("code_verifier", &self.verifier),
                ],
                None,
                store,
            )
            .await
    }
    async fn callback(&self) -> Result<Zeroizing<String>, ConnectionError> {
        loop {
            let (mut stream, _) = self
                .listener
                .accept()
                .await
                .map_err(|_| ConnectionError::Transport)?;
            let deadline = self.deadline.min(Instant::now() + Duration::from_secs(5));
            let read = timeout_at(deadline, async {
                let mut buf = Zeroizing::new(Vec::new());
                let mut byte = [0u8; 1];
                while buf.len() < 8192 && !buf.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).await.ok()? == 0 {
                        return None;
                    }
                    buf.push(byte[0]);
                }
                if !buf.ends_with(b"\r\n\r\n") {
                    return None;
                }
                let req = std::str::from_utf8(&buf).ok()?;
                let line = req.lines().next()?;
                let mut parts = line.split_whitespace();
                if parts.next()? != "GET" {
                    return None;
                }
                let target = parts.next()?;
                if parts.next()? != "HTTP/1.1"
                    || parts.next().is_some()
                    || !target.starts_with("/callback?")
                {
                    return None;
                }
                let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
                let mut state = None;
                let mut code = None;
                for (k, v) in url.query_pairs() {
                    match k.as_ref() {
                        "state" if state.is_none() => state = Some(v.into_owned()),
                        "code" if code.is_none() => code = Some(v.into_owned()),
                        "state" | "code" => return None,
                        _ => (),
                    }
                }
                let state = state?;
                let code = Zeroizing::new(code?);
                if state.as_bytes().ct_eq(self.state.as_bytes()).unwrap_u8() != 1
                    || !valid_secret(&code)
                {
                    return None;
                }
                Some(code)
            })
            .await
            .ok()
            .flatten();
            let (status, body) = if read.is_some() {
                (
                    "200 OK",
                    "Authorization complete. You may close this window.",
                )
            } else {
                ("400 Bad Request", "Invalid authorization callback.")
            };
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",body.len());
            let _ = timeout_at(deadline, stream.write_all(response.as_bytes())).await;
            if let Some(code) = read {
                return Ok(code);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryCredentialStore;
    use std::{
        io::{Read, Write},
        net::TcpListener as StdListener,
        thread,
    };
    fn server(responses: Vec<&'static str>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = StdListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for body in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut b = [0; 1];
                while !bytes.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut b).unwrap();
                    bytes.push(b[0]);
                }
                let head = String::from_utf8(bytes).unwrap();
                let length: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.parse().ok())
                    })
                    .unwrap();
                let mut data = vec![0; length];
                stream.read_exact(&mut data).unwrap();
                requests.push(head + &String::from_utf8(data).unwrap());
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            }
            requests
        });
        (format!("http://{address}/"), handle)
    }
    async fn callback(url: &Url, state: &str, code: &str) -> String {
        let params: std::collections::HashMap<_, _> =
            url::form_urlencoded::parse(url.fragment().unwrap().as_bytes())
                .into_owned()
                .collect();
        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", params["port"]))
            .await
            .unwrap();
        let target = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("state", state)
            .append_pair("code", code)
            .finish();
        stream
            .write_all(
                format!("GET /callback?{target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes(),
            )
            .await
            .unwrap();
        let mut result = String::new();
        stream.read_to_string(&mut result).await.unwrap();
        result
    }
    #[tokio::test]
    async fn login_callback_exchange_refresh_restart_and_redaction() {
        let (origin, server) = server(vec![
            r#"{"access_token":"access-one","refresh_token":"refresh-one","token_type":"Bearer","expires_in":3600}"#,
            r#"{"access_token":"access-two","refresh_token":"refresh-two","token_type":"Bearer","expires_in":3600}"#,
        ]);
        let config = CloudConfig::loopback_fixture(&origin, &origin).unwrap();
        let store = MemoryCredentialStore::default();
        let (url, pending) = config.begin_login().await.unwrap();
        let params: std::collections::HashMap<_, _> =
            url::form_urlencoded::parse(url.fragment().unwrap().as_bytes())
                .into_owned()
                .collect();
        assert_eq!(url.path(), "/connect");
        assert_eq!(params["challenge"].len(), 43);
        let state = params["state"].clone();
        let future = async {
            assert!(callback(&url, "wrong", "bad-code")
                .await
                .starts_with("HTTP/1.1 400"));
            assert!(callback(&url, &state, "fixture-code")
                .await
                .starts_with("HTTP/1.1 200"));
        };
        let (grant, ()) = tokio::join!(pending.complete(&config, &store), future);
        let grant = grant.unwrap();
        assert!(!format!("{grant:?}").contains("access-one"));
        let id = grant.cache_key().to_owned();
        let loaded = config.load(&store).unwrap().unwrap();
        assert_eq!(loaded.cache_key(), id);
        let refreshed = config.refresh(&loaded, &store).await.unwrap();
        assert_eq!(refreshed.cache_key(), id);
        assert_eq!(
            config.load(&store).unwrap().unwrap().access_token(),
            "access-two"
        );
        let requests = server.join().unwrap();
        assert!(requests[0].contains("grant_type=authorization_code"));
        assert!(requests[0].contains("code=fixture-code"));
        let body = requests[0].split("\r\n\r\n").nth(1).unwrap();
        let form: std::collections::HashMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(
            URL_SAFE_NO_PAD.encode(Sha256::digest(form["code_verifier"].as_bytes())),
            params["challenge"]
        );
        assert_eq!(
            form["redirect_uri"],
            format!("http://127.0.0.1:{}/callback", params["port"])
        );
        assert!(requests[1].contains("refresh_token=refresh-one"));
        config.disconnect(&store).unwrap();
        assert!(config.load(&store).unwrap().is_none());
    }
    #[tokio::test]
    async fn invalid_token_never_saved_and_http_configuration_restricted() {
        assert!(CloudConfig::loopback_fixture("http://example.com/", "http://127.0.0.1/").is_err());
        let (origin, server) = server(vec![
            r#"{"access_token":"x","refresh_token":"y","token_type":"DPoP","expires_in":3600}"#,
        ]);
        let config = CloudConfig::loopback_fixture(&origin, &origin).unwrap();
        let store = MemoryCredentialStore::default();
        let (url, pending) = config.begin_login().await.unwrap();
        let state = url::form_urlencoded::parse(url.fragment().unwrap().as_bytes())
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let (result, _) = tokio::join!(
            pending.complete(&config, &store),
            callback(&url, &state, "code")
        );
        assert!(matches!(result, Err(ConnectionError::Protocol)));
        assert!(config.load(&store).unwrap().is_none());
        server.join().unwrap();
    }
    #[tokio::test]
    async fn expired_callback_and_invalid_tokens_do_not_publish() {
        let (origin, server) = server(vec![
            r#"{"access_token":"x","refresh_token":"y","token_type":"Bearer","expires_in":0}"#,
        ]);
        let config = CloudConfig::loopback_fixture(&origin, &origin).unwrap();
        let store = MemoryCredentialStore::default();
        let (_, mut expired) = config.begin_login().await.unwrap();
        expired.deadline = Instant::now();
        assert!(matches!(
            expired.complete(&config, &store).await,
            Err(ConnectionError::PermissionDenied)
        ));
        let (url, pending) = config.begin_login().await.unwrap();
        let state = url::form_urlencoded::parse(url.fragment().unwrap().as_bytes())
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let (result, _) = tokio::join!(
            pending.complete(&config, &store),
            callback(&url, &state, "code")
        );
        assert!(matches!(result, Err(ConnectionError::Protocol)));
        assert!(config.load(&store).unwrap().is_none());
        server.join().unwrap();
    }
}
