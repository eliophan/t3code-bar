pub mod catalog;
pub mod cloud;
pub mod cloud_session;
pub mod credentials;
pub mod dpop;
pub mod relay;
pub mod rpc;
pub mod ssh;

// Direct T3 environment connection boundary.
//
// The descriptor is public. Credentials are sent only after it identifies the
// expected environment, and redirects are never followed.

use bar_identity::{EnvironmentId, EnvironmentMachineKind};
use reqwest::{Client, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};
use std::{
    fmt,
    net::IpAddr,
    time::{Duration, Instant, SystemTime},
};
use url::Url;
use zeroize::Zeroizing;

#[derive(Debug, PartialEq, Eq)]
pub enum ConnectionError {
    InvalidRoute,
    WrongEnvironment,
    InvalidDescriptor,
    PairingRejected,
    PermissionDenied,
    /// A saved grant passed its local expiry. The server did not reject it.
    GrantExpired,
    Protocol,
    Transport,
    Storage,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread,
    };

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
        let length = head
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|s| s.parse::<usize>().ok())
            })
            .unwrap_or(0);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        head + &String::from_utf8(body).unwrap()
    }

    fn respond(stream: &mut TcpStream, body: &str) {
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    }

    fn descriptor(id: &str) -> String {
        format!(
            r#"{{"environmentId":"{id}","label":"Test","platform":{{"os":"linux","arch":"x64","machine":"mac-mini"}},"serverVersion":"1","capabilities":{{}}}}"#
        )
    }

    #[tokio::test]
    async fn wrong_identity_is_rejected_before_any_credential() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let head = request(&mut stream);
            assert!(head.starts_with("GET /.well-known/t3/environment "));
            assert!(!head.to_ascii_lowercase().contains("authorization:"));
            respond(&mut stream, &descriptor("different"));
        });
        assert!(matches!(
            VerifiedEndpoint::discover(
                &format!("http://{addr}/"),
                &EnvironmentId::new("expected").unwrap()
            )
            .await,
            Err(ConnectionError::WrongEnvironment)
        ));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn changed_identity_cannot_receive_pairing_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for id in ["env-a", "env-b"] {
                let (mut stream, _) = listener.accept().unwrap();
                let input = request(&mut stream);
                assert!(input.starts_with("GET /.well-known/t3/environment "));
                assert!(!input.contains("ABCD2345EFGH"));
                respond(&mut stream, &descriptor(id));
            }
        });
        let endpoint = VerifiedEndpoint::discover(
            &format!("http://{addr}/"),
            &EnvironmentId::new("env-a").unwrap(),
        )
        .await
        .unwrap();
        assert!(matches!(
            endpoint.pair("ABCD2345EFGH").await,
            Err(ConnectionError::WrongEnvironment)
        ));
        server.join().unwrap();
    }

    #[tokio::test]
    async fn pairing_requests_read_scopes_and_ticket_uses_own_grant() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for step in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let input = request(&mut stream);
                match step {
                    0 | 1 | 3 => {
                        assert!(input.starts_with("GET /.well-known/t3/environment "));
                        assert!(!input.to_ascii_lowercase().contains("authorization:"));
                        respond(&mut stream, &descriptor("env-a"));
                    }
                    2 => {
                        assert!(input.starts_with("POST /oauth/token "));
                        assert!(input.contains("scope=orchestration%3Aread+diagnostics%3Aread"));
                        respond(
                            &mut stream,
                            r#"{"access_token":"synthetic-token","token_type":"Bearer","expires_in":3600,"scope":"orchestration:read"}"#,
                        );
                    }
                    4 => {
                        assert!(input.starts_with("POST /api/auth/websocket-ticket/rpc "));
                        assert!(input
                            .to_ascii_lowercase()
                            .contains("authorization: bearer synthetic-token"));
                        respond(
                            &mut stream,
                            r#"{"ticket":"synthetic-ticket","expiresAt":"2030-01-01T00:00:00Z"}"#,
                        );
                    }
                    _ => unreachable!(),
                }
            }
        });
        let endpoint = VerifiedEndpoint::discover(
            &format!("http://{addr}/"),
            &EnvironmentId::new("env-a").unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            endpoint.descriptor().machine,
            Some(EnvironmentMachineKind::MacMini)
        );
        let grant = endpoint.pair("ABCD2345EFGH").await.unwrap();
        assert!(grant.orchestration_read);
        assert!(!grant.diagnostics_read);
        assert!(!format!("{grant:?}").contains("synthetic-token"));
        assert_eq!(
            endpoint.websocket_ticket(&grant).await.unwrap().as_str(),
            "synthetic-ticket"
        );
        server.join().unwrap();
    }

    #[tokio::test]
    async fn unencrypted_remote_and_url_credentials_are_rejected() {
        let id = EnvironmentId::new("env-a").unwrap();
        for route in [
            "http://example.com/",
            "https://user:pass@example.com/",
            "https://example.com/path",
        ] {
            assert!(matches!(
                VerifiedEndpoint::discover(route, &id).await,
                Err(ConnectionError::InvalidRoute)
            ));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentDescriptor {
    pub id: EnvironmentId,
    pub label: String,
    pub machine: Option<EnvironmentMachineKind>,
    pub server_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DescriptorWire {
    environment_id: EnvironmentId,
    label: String,
    platform: PlatformWire,
    server_version: String,
    capabilities: serde_json::Map<String, serde_json::Value>,
}

#[derive(Deserialize)]
struct PlatformWire {
    os: String,
    arch: String,
    #[serde(default)]
    machine: Option<serde_json::Value>,
}

/// An endpoint bound to the environment identity obtained without credentials.
pub struct VerifiedEndpoint {
    client: Client,
    base: Url,
    ws_base: Url,
    descriptor: EnvironmentDescriptor,
    /// Stable credential identity for transports whose local origin is
    /// ephemeral. Only `ssh::SshTunnel::verified_endpoint` sets it.
    credential_identity: Option<String>,
}

/// A Bar-owned grant, kept secret when formatted or dropped.
pub struct Grant {
    environment: EnvironmentId,
    token: Zeroizing<String>,
    origin: String,
    expires_at: Instant,
    expires_wall: SystemTime,
    pub orchestration_read: bool,
    pub diagnostics_read: bool,
    dpop: Option<crate::dpop::DpopSigner>,
}

async fn decode_response<T: DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, ConnectionError> {
    const MAX_RESPONSE: usize = 1024 * 1024;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE as u64)
    {
        return Err(ConnectionError::Protocol);
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ConnectionError::Transport)?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE {
            return Err(ConnectionError::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| ConnectionError::Protocol)
}

impl Grant {
    /// Opaque grant identity for invalidating session caches. It is not an account ID.
    pub fn principal_identity(&self) -> credentials::PrincipalIdentity {
        credentials::PrincipalIdentity::from_token(&self.token)
    }

    pub fn expires_at(&self) -> Instant {
        self.expires_at
    }

    pub(crate) fn access_token(&self) -> &str {
        &self.token
    }

    pub(crate) fn dpop_signer(&self) -> Option<&crate::dpop::DpopSigner> {
        self.dpop.as_ref()
    }
}

impl fmt::Debug for Grant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Grant")
            .field("token", &"[redacted]")
            .field("orchestration_read", &self.orchestration_read)
            .field("diagnostics_read", &self.diagnostics_read)
            .finish()
    }
}

fn validated_base(route: &str) -> Result<Url, ConnectionError> {
    let base = Url::parse(route).map_err(|_| ConnectionError::InvalidRoute)?;
    let loopback = base
        .host_str()
        .and_then(|h| h.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if !(base.scheme() == "https" || base.scheme() == "http" && loopback)
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.path() != "/"
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(ConnectionError::InvalidRoute);
    }
    Ok(base)
}

fn valid_secret(s: &str) -> bool {
    !s.is_empty() && s.len() <= 16384 && !s.bytes().any(|b| b < 33 || b == 127)
}

impl VerifiedEndpoint {
    pub async fn discover(route: &str, expected: &EnvironmentId) -> Result<Self, ConnectionError> {
        let base = validated_base(route)?;
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| ConnectionError::Transport)?;
        let response = client
            .get(base.join("/.well-known/t3/environment").unwrap())
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        if !response.status().is_success() {
            return Err(ConnectionError::Transport);
        }
        let wire: DescriptorWire =
            decode_response(response)
                .await
                .map_err(|error| match error {
                    ConnectionError::Protocol => ConnectionError::InvalidDescriptor,
                    other => other,
                })?;
        if wire.environment_id != *expected {
            return Err(ConnectionError::WrongEnvironment);
        }
        if wire.label.trim().is_empty()
            || wire.server_version.trim().is_empty()
            || !["darwin", "linux", "windows", "unknown"].contains(&wire.platform.os.as_str())
            || !["arm64", "x64", "other"].contains(&wire.platform.arch.as_str())
        {
            return Err(ConnectionError::InvalidDescriptor);
        }
        let _ = wire.capabilities;
        let machine = wire
            .platform
            .machine
            .and_then(|v| serde_json::from_value(v).ok());
        let mut ws_base = base.clone();
        ws_base
            .set_scheme(if base.scheme() == "https" {
                "wss"
            } else {
                "ws"
            })
            .map_err(|_| ConnectionError::InvalidRoute)?;
        Ok(Self {
            client,
            base,
            ws_base,
            descriptor: EnvironmentDescriptor {
                id: wire.environment_id,
                label: wire.label.trim().to_owned(),
                machine,
                server_version: wire.server_version.trim().to_owned(),
            },
            credential_identity: None,
        })
    }

    pub fn descriptor(&self) -> &EnvironmentDescriptor {
        &self.descriptor
    }

    pub(crate) fn set_ws_base(&mut self, route: &str) -> Result<(), ConnectionError> {
        let ws = Url::parse(route).map_err(|_| ConnectionError::InvalidRoute)?;
        if !matches!(ws.scheme(), "ws" | "wss")
            || ws.host_str().is_none()
            || !ws.username().is_empty()
            || ws.password().is_some()
            || ws.query().is_some()
            || ws.fragment().is_some()
            || ws.host_str() != self.base.host_str()
            || ws.port_or_known_default() != self.base.port_or_known_default()
            || (ws.scheme() == "wss") != (self.base.scheme() == "https")
        {
            return Err(ConnectionError::InvalidRoute);
        }
        self.ws_base = ws;
        Ok(())
    }

    pub async fn pair(&self, code: &str) -> Result<Grant, ConnectionError> {
        let code = Zeroizing::new(code.trim().to_ascii_uppercase());
        if code.len() != 12
            || !code
                .chars()
                .all(|c| "23456789ABCDEFGHJKLMNPQRSTUVWXYZ".contains(c))
        {
            return Err(ConnectionError::PairingRejected);
        }
        Self::discover(self.base.as_str(), &self.descriptor.id).await?;
        let requested_at = Instant::now();
        let requested_wall = SystemTime::now();
        let response = self
            .client
            .post(self.base.join("/oauth/token").unwrap())
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:token-exchange",
                ),
                ("subject_token", code.as_str()),
                (
                    "subject_token_type",
                    "urn:t3:params:oauth:token-type:environment-bootstrap",
                ),
                (
                    "requested_token_type",
                    "urn:ietf:params:oauth:token-type:access_token",
                ),
                ("client_label", "T3 Code Bar"),
                ("scope", "orchestration:read diagnostics:read"),
                ("client_device_type", "desktop"),
                ("client_os", std::env::consts::OS),
            ])
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        match response.status() {
            StatusCode::OK => (),
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(ConnectionError::PairingRejected)
            }
            _ => return Err(ConnectionError::Transport),
        }
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
            token_type: String,
            expires_in: f64,
            scope: String,
        }
        let token: Token = decode_response(response).await?;
        let secret = Zeroizing::new(token.access_token);
        if token.token_type != "Bearer"
            || !token.expires_in.is_finite()
            || token.expires_in <= 0.0
            || secret.is_empty()
            || secret.len() > 16384
            || secret.bytes().any(|b| b < 32 || b == 127)
        {
            return Err(ConnectionError::Protocol);
        }
        let duration =
            Duration::try_from_secs_f64(token.expires_in).map_err(|_| ConnectionError::Protocol)?;
        let expires_at = requested_at
            .checked_add(duration)
            .ok_or(ConnectionError::Protocol)?;
        let expires_wall = requested_wall
            .checked_add(duration)
            .ok_or(ConnectionError::Protocol)?;
        let scopes: Vec<_> = token.scope.split_whitespace().collect();
        if scopes.is_empty()
            || scopes
                .iter()
                .any(|scope| !["orchestration:read", "diagnostics:read"].contains(scope))
        {
            return Err(ConnectionError::Protocol);
        }
        Ok(Grant {
            environment: self.descriptor.id.clone(),
            token: secret,
            origin: self.base.as_str().to_owned(),
            expires_at,
            expires_wall,
            orchestration_read: scopes.contains(&"orchestration:read"),
            diagnostics_read: scopes.contains(&"diagnostics:read"),
            dpop: None,
        })
    }

    pub(crate) async fn bootstrap(
        &self,
        credential: &str,
        signer: crate::dpop::DpopSigner,
    ) -> Result<Grant, ConnectionError> {
        if credential.is_empty() || credential.len() > 16384 || !valid_secret(credential) {
            return Err(ConnectionError::Protocol);
        }
        Self::discover(self.base.as_str(), &self.descriptor.id).await?;
        let token_url = self
            .base
            .join("/oauth/token")
            .map_err(|_| ConnectionError::InvalidRoute)?;
        let proof = signer.proof("POST", token_url.as_str(), None)?;
        let requested_at = Instant::now();
        let requested_wall = SystemTime::now();
        let response = self
            .client
            .post(token_url)
            .header("DPoP", proof)
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:token-exchange",
                ),
                ("subject_token", credential),
                (
                    "subject_token_type",
                    "urn:t3:params:oauth:token-type:environment-bootstrap",
                ),
                (
                    "requested_token_type",
                    "urn:ietf:params:oauth:token-type:access_token",
                ),
                ("scope", "orchestration:read diagnostics:read"),
            ])
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        match response.status() {
            StatusCode::OK => (),
            StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(ConnectionError::PairingRejected)
            }
            _ => return Err(ConnectionError::Transport),
        }
        #[derive(Deserialize)]
        struct Token {
            access_token: String,
            token_type: String,
            issued_token_type: String,
            expires_in: f64,
            scope: String,
        }
        let token: Token = decode_response(response).await?;
        let secret = Zeroizing::new(token.access_token);
        if !token.token_type.eq_ignore_ascii_case("DPoP")
            || token.issued_token_type != "urn:ietf:params:oauth:token-type:access_token"
            || !token.expires_in.is_finite()
            || token.expires_in <= 0.0
            || token.expires_in.fract() != 0.0
            || token.expires_in > 315_360_000.0
            || secret.is_empty()
            || secret.len() > 16384
            || secret.bytes().any(|b| b < 32 || b == 127)
        {
            return Err(ConnectionError::Protocol);
        }
        let scopes: Vec<_> = token.scope.split_whitespace().collect();
        if scopes.is_empty()
            || scopes
                .iter()
                .any(|scope| !["orchestration:read", "diagnostics:read"].contains(scope))
        {
            return Err(ConnectionError::Protocol);
        }
        let duration =
            Duration::try_from_secs_f64(token.expires_in).map_err(|_| ConnectionError::Protocol)?;
        Ok(Grant {
            environment: self.descriptor.id.clone(),
            token: secret,
            origin: self.base.as_str().to_owned(),
            expires_at: requested_at
                .checked_add(duration)
                .ok_or(ConnectionError::Protocol)?,
            expires_wall: requested_wall
                .checked_add(duration)
                .ok_or(ConnectionError::Protocol)?,
            orchestration_read: scopes.contains(&"orchestration:read"),
            diagnostics_read: scopes.contains(&"diagnostics:read"),
            dpop: Some(signer),
        })
    }

    /// Obtain a short-lived RPC ticket for this endpoint and grant.
    pub(crate) async fn websocket_ticket(
        &self,
        grant: &Grant,
    ) -> Result<Zeroizing<String>, ConnectionError> {
        if grant.environment != self.descriptor.id {
            return Err(ConnectionError::WrongEnvironment);
        }
        if grant.origin != self.base.as_str() {
            return Err(ConnectionError::InvalidRoute);
        }
        if Instant::now() >= grant.expires_at || SystemTime::now() >= grant.expires_wall {
            return Err(ConnectionError::PermissionDenied);
        }
        Self::discover(self.base.as_str(), &self.descriptor.id).await?;
        let ticket_url = self
            .base
            .join("/api/auth/websocket-ticket/rpc")
            .map_err(|_| ConnectionError::InvalidRoute)?;
        let mut request = self.client.post(ticket_url.clone());
        if let Some(signer) = grant.dpop_signer() {
            request = request
                .header("Authorization", format!("DPoP {}", grant.access_token()))
                .header(
                    "DPoP",
                    signer.proof("POST", ticket_url.as_str(), Some(grant.access_token()))?,
                );
        } else {
            request = request.bearer_auth(grant.access_token());
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        if response.status() == StatusCode::NOT_FOUND {
            let fallback_url = self
                .base
                .join("/api/auth/websocket-ticket")
                .map_err(|_| ConnectionError::InvalidRoute)?;
            let mut request = self.client.post(fallback_url.clone());
            if let Some(signer) = grant.dpop_signer() {
                request = request
                    .header("Authorization", format!("DPoP {}", grant.access_token()))
                    .header(
                        "DPoP",
                        signer.proof("POST", fallback_url.as_str(), Some(grant.access_token()))?,
                    );
            } else {
                request = request.bearer_auth(grant.access_token());
            }
            response = request
                .send()
                .await
                .map_err(|_| ConnectionError::Transport)?;
        }
        match response.status() {
            StatusCode::OK => (),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(ConnectionError::PermissionDenied)
            }
            _ => return Err(ConnectionError::Transport),
        }
        #[derive(Deserialize)]
        struct Ticket {
            ticket: String,
            #[serde(rename = "expiresAt")]
            expires_at: String,
        }
        let ticket: Ticket = decode_response(response).await?;
        let expires = chrono::DateTime::parse_from_rfc3339(&ticket.expires_at)
            .map_err(|_| ConnectionError::Protocol)?;
        if !valid_secret(&ticket.ticket) || expires <= chrono::Utc::now() {
            return Err(ConnectionError::Protocol);
        }
        Ok(Zeroizing::new(ticket.ticket))
    }

    pub(crate) fn websocket_url(
        &self,
        grant: &Grant,
        ticket: &str,
    ) -> Result<Url, ConnectionError> {
        if grant.environment != self.descriptor.id || grant.origin != self.base.as_str() {
            return Err(ConnectionError::WrongEnvironment);
        }
        let mut url = self.ws_base.clone();
        if url.path().is_empty() || url.path() == "/" {
            url.set_path("/ws");
        }
        url.query_pairs_mut().append_pair("wsTicket", ticket);
        if let Some(signer) = grant.dpop_signer() {
            let mut http_url = url.clone();
            http_url
                .set_scheme(if url.scheme() == "wss" {
                    "https"
                } else {
                    "http"
                })
                .map_err(|_| ConnectionError::InvalidRoute)?;
            http_url.set_query(None);
            http_url.set_fragment(None);
            let proof = signer.proof("GET", http_url.as_str(), Some(ticket))?;
            url.query_pairs_mut().append_pair("dpop", &proof);
        }
        Ok(url)
    }
}
