//! Relay authorization and environment connection.
//!
//! The relay token and the environment bootstrap credential are separate
//! grants. Both are bound to the Bar-owned DPoP key and are checked before
//! any environment credential is sent.

use crate::{
    catalog::{EnvironmentMetadata, Route},
    cloud::{CloudConfig, CloudGrant},
    credentials::{CredentialScope, CredentialStore},
    decode_response,
    dpop::DpopSigner,
    ConnectionError, Grant, VerifiedEndpoint,
};
use bar_identity::EnvironmentId;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

const RELAY_CONNECT_SCOPE: &str = "environment:connect";
const CLIENT_ID: &str = "t3-web";
const TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const JWT_TYPE: &str = "urn:ietf:params:oauth:token-type:jwt";
const ACCESS_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayEndpoint {
    pub http_base_url: String,
    pub ws_base_url: String,
    pub provider_kind: String,
}

pub struct RelayConnection {
    pub endpoint: VerifiedEndpoint,
    pub grant: Grant,
}

pub struct RelayClient {
    origin: Url,
    client: Client,
}

impl RelayClient {
    pub fn production() -> Result<Self, ConnectionError> {
        Self::new("https://relay.t3.codes/")
    }

    pub fn loopback_fixture(origin: &str) -> Result<Self, ConnectionError> {
        let url = Url::parse(origin).map_err(|_| ConnectionError::InvalidRoute)?;
        if url.scheme() != "http"
            || !url
                .host_str()
                .and_then(|host| host.parse::<IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback())
        {
            return Err(ConnectionError::InvalidRoute);
        }
        Self::from_url(url)
    }

    fn new(origin: &str) -> Result<Self, ConnectionError> {
        let url = Url::parse(origin).map_err(|_| ConnectionError::InvalidRoute)?;
        if url.scheme() != "https" {
            return Err(ConnectionError::InvalidRoute);
        }
        Self::from_url(url)
    }

    fn from_url(origin: Url) -> Result<Self, ConnectionError> {
        if origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            return Err(ConnectionError::InvalidRoute);
        }
        Ok(Self {
            origin,
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .user_agent("T3-Code-Bar/2.0.0-alpha.1")
                .build()
                .map_err(|_| ConnectionError::Transport)?,
        })
    }

    pub fn origin(&self) -> &str {
        self.origin.as_str()
    }

    /// OAuth authorizes directory lookup. Each environment still requires a
    /// separate Bar pairing grant; the cloud bearer is never sent to its tunnel.
    pub async fn list_environments(
        &self,
        cloud: &CloudGrant,
    ) -> Result<Vec<EnvironmentMetadata>, ConnectionError> {
        if cloud.expires_unix_seconds() <= now_unix() {
            return Err(ConnectionError::PermissionDenied);
        }
        let response = self
            .client
            .get(
                self.origin
                    .join("v1/environments")
                    .map_err(|_| ConnectionError::InvalidRoute)?,
            )
            .bearer_auth(cloud.access_token())
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        match response.status() {
            StatusCode::OK => (),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(ConnectionError::PermissionDenied)
            }
            _ => return Err(ConnectionError::Transport),
        }
        let directory: DirectoryResponse = decode_response(response).await?;
        directory_entries(directory, self.origin.scheme() == "http")
    }

    pub async fn connect(
        &self,
        environment: &EnvironmentId,
        cloud: &CloudGrant,
        signer: &DpopSigner,
        store: &dyn CredentialStore,
    ) -> Result<RelayConnection, ConnectionError> {
        if cloud.expires_unix_seconds() <= now_unix() {
            return Err(ConnectionError::PermissionDenied);
        }
        let relay = self.relay_token(environment, cloud, signer, store).await?;
        let mut url = self.origin.clone();
        url.path_segments_mut()
            .map_err(|_| ConnectionError::InvalidRoute)?
            .clear()
            .extend(["v1", "environments", environment.as_str(), "connect"]);
        let proof = signer.proof("POST", url.as_str(), Some(&relay.token))?;
        let response = self
            .client
            .post(url)
            .header("Authorization", format!("DPoP {}", relay.token.as_str()))
            .header("DPoP", proof)
            .json(&serde_json::json!({ "clientKeyThumbprint": signer.thumbprint() }))
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        match response.status() {
            StatusCode::OK => (),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(ConnectionError::PermissionDenied)
            }
            _ => return Err(ConnectionError::Transport),
        }
        let response: ConnectResponse = decode_response(response).await?;
        if response.environment_id != *environment {
            return Err(ConnectionError::WrongEnvironment);
        }
        if !valid_secret(&response.credential) || response.expires_at.trim().is_empty() {
            return Err(ConnectionError::Protocol);
        }
        let expires: chrono::DateTime<chrono::Utc> = response
            .expires_at
            .parse()
            .map_err(|_| ConnectionError::Protocol)?;
        if expires <= chrono::Utc::now() {
            return Err(ConnectionError::Protocol);
        }
        validate_endpoint(&response.endpoint)?;
        let mut endpoint =
            VerifiedEndpoint::discover(&response.endpoint.http_base_url, environment).await?;
        endpoint.set_ws_base(&response.endpoint.ws_base_url)?;
        let grant_scope = CredentialScope::relay_environment_grant(
            &response.endpoint.http_base_url,
            environment,
            cloud.cache_key(),
            &signer.thumbprint(),
        );
        match endpoint.load_saved_grant_with_signer_at_scope(store, signer, &grant_scope) {
            Ok(Some(grant))
                if grant.expires_at() > std::time::Instant::now() + Duration::from_secs(15) =>
            {
                return Ok(RelayConnection { endpoint, grant });
            }
            Ok(_) | Err(ConnectionError::PermissionDenied) => (),
            Err(error) => return Err(error),
        }
        let grant = endpoint
            .bootstrap(&response.credential, signer.clone())
            .await?;
        endpoint.save_grant_at_scope(&grant, store, &grant_scope)?;
        Ok(RelayConnection { endpoint, grant })
    }

    /// Refresh a nearly expired cloud session before connecting. The caller keeps
    /// the returned cloud grant for later connections and cache invalidation.
    pub async fn connect_refreshing(
        &self,
        environment: &EnvironmentId,
        config: &CloudConfig,
        cloud: CloudGrant,
        store: &dyn CredentialStore,
    ) -> Result<(RelayConnection, CloudGrant), ConnectionError> {
        let cloud = if cloud.expires_unix_seconds() <= now_unix().saturating_add(60) {
            config.refresh(&cloud, store).await?
        } else {
            cloud
        };
        let signer = config.signer(store)?;
        let connection = self.connect(environment, &cloud, &signer, store).await?;
        Ok((connection, cloud))
    }

    async fn relay_token(
        &self,
        environment: &EnvironmentId,
        cloud: &CloudGrant,
        signer: &DpopSigner,
        store: &dyn CredentialStore,
    ) -> Result<RelayToken, ConnectionError> {
        let scope = CredentialScope::relay_access(
            self.origin.as_str(),
            environment,
            cloud.cache_key(),
            &signer.thumbprint(),
        );
        if let Some(bytes) = store.load(&scope)? {
            if let Ok(mut saved) = serde_json::from_slice::<StoredRelayToken>(&bytes) {
                if saved.version == 1
                    && saved.origin == self.origin.as_str()
                    && saved.environment == *environment
                    && saved.principal == cloud.cache_key()
                    && saved.thumbprint == signer.thumbprint()
                    && saved.expires > now_unix() + 15
                    && valid_secret(&saved.token)
                    && exact_scope(&saved.scope)
                {
                    return Ok(RelayToken {
                        token: Zeroizing::new(std::mem::take(&mut saved.token)),
                    });
                }
            }
        }
        let url = self
            .origin
            .join("v1/client/dpop-token")
            .map_err(|_| ConnectionError::InvalidRoute)?;
        let proof = signer.proof("POST", url.as_str(), None)?;
        let requested_at = now_unix();
        let resource = self.origin.origin().ascii_serialization();
        let response = self
            .client
            .post(url)
            .header("DPoP", proof)
            .form(&[
                ("grant_type", TOKEN_EXCHANGE),
                ("subject_token", cloud.access_token()),
                ("subject_token_type", JWT_TYPE),
                ("requested_token_type", ACCESS_TYPE),
                ("resource", resource.as_str()),
                ("scope", RELAY_CONNECT_SCOPE),
                ("client_id", CLIENT_ID),
            ])
            .send()
            .await
            .map_err(|_| ConnectionError::Transport)?;
        if !response.status().is_success() {
            let status = response.status();
            if status == StatusCode::UNAUTHORIZED {
                #[derive(Deserialize)]
                struct Rejection {
                    #[serde(rename = "_tag")]
                    tag: String,
                    reason: String,
                }
                if decode_response::<Rejection>(response).await.is_ok_and(|error| {
                    error.tag == "RelayAuthInvalidError" && error.reason == "invalid_bearer"
                }) {
                    return Err(ConnectionError::RelayCredentialUnsupported);
                }
            }
            return Err(if status.is_client_error() {
                ConnectionError::PermissionDenied
            } else {
                ConnectionError::Transport
            });
        }
        let mut token: RelayTokenResponse = decode_response(response).await?;
        if token.token_type != "DPoP"
            || token.issued_token_type != ACCESS_TYPE
            || !token.expires_in.is_finite()
            || token.expires_in.fract() != 0.0
            || token.expires_in <= 0.0
            || token.expires_in > 315_360_000.0
            || !valid_secret(&token.access_token)
            || !exact_scope(&token.scope)
        {
            return Err(ConnectionError::Protocol);
        }
        let expires = requested_at
            .checked_add(token.expires_in as u64)
            .ok_or(ConnectionError::Protocol)?;
        let saved = StoredRelayToken {
            version: 1,
            origin: self.origin.to_string(),
            environment: environment.clone(),
            principal: cloud.cache_key().to_owned(),
            thumbprint: signer.thumbprint(),
            token: token.access_token.clone(),
            expires,
            scope: token.scope.clone(),
        };
        let bytes =
            Zeroizing::new(serde_json::to_vec(&saved).map_err(|_| ConnectionError::Storage)?);
        store.save(&scope, &bytes)?;
        Ok(RelayToken {
            token: Zeroizing::new(std::mem::take(&mut token.access_token)),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryRecord {
    environment_id: EnvironmentId,
    label: String,
    endpoint: RelayEndpoint,
}

#[derive(Deserialize)]
struct DirectoryResponse {
    environments: Vec<DirectoryRecord>,
}

fn directory_entries(
    directory: DirectoryResponse,
    fixture: bool,
) -> Result<Vec<EnvironmentMetadata>, ConnectionError> {
    if directory.environments.len() > 128 {
        return Err(ConnectionError::Protocol);
    }
    let mut entries: Vec<EnvironmentMetadata> = Vec::new();
    for record in directory.environments {
        validate_endpoint(&record.endpoint)?;
        let origin = crate::normalized_origin(&record.endpoint.http_base_url)?;
        if (!fixture && origin.scheme() != "https")
            || (fixture
                && origin.scheme() == "http"
                && !origin
                    .host_str()
                    .and_then(|host| host.parse::<IpAddr>().ok())
                    .is_some_and(|ip| ip.is_loopback()))
            || record.label.trim().is_empty()
            || record.label.len() > 256
        {
            return Err(ConnectionError::InvalidDescriptor);
        }
        let route = Route::Bearer {
            connection_id: "relay-directory".into(),
            http_base_url: Some(origin.to_string()),
        };
        if entries.iter().any(|entry| {
            entry.id == record.environment_id
                || entry
                    .routes
                    .iter()
                    .any(|known| crate::manual::same_destination(known, &route))
        }) {
            return Err(ConnectionError::InvalidDescriptor);
        }
        entries.push(EnvironmentMetadata {
            id: record.environment_id,
            label: record.label,
            enabled: true,
            routes: vec![route],
        });
    }
    Ok(entries)
}

fn validate_endpoint(endpoint: &RelayEndpoint) -> Result<(), ConnectionError> {
    let http =
        Url::parse(&endpoint.http_base_url).map_err(|_| ConnectionError::InvalidDescriptor)?;
    let ws = Url::parse(&endpoint.ws_base_url).map_err(|_| ConnectionError::InvalidDescriptor)?;
    if !matches!(http.scheme(), "http" | "https")
        || !matches!(ws.scheme(), "ws" | "wss")
        || http.host_str().is_none()
        || ws.host_str().is_none()
        || !http.username().is_empty()
        || http.password().is_some()
        || !ws.username().is_empty()
        || ws.password().is_some()
        || http.query().is_some()
        || http.fragment().is_some()
        || ws.query().is_some()
        || ws.fragment().is_some()
        || ws.host_str() != http.host_str()
        || ws.port_or_known_default() != http.port_or_known_default()
        || (ws.scheme() == "wss") != (http.scheme() == "https")
        || !["manual", "cloudflare_tunnel", "t3_relay"].contains(&endpoint.provider_kind.as_str())
    {
        return Err(ConnectionError::InvalidDescriptor);
    }
    Ok(())
}

fn exact_scope(scope: &str) -> bool {
    let mut values = scope.split_whitespace().collect::<Vec<_>>();
    values.sort_unstable();
    values == [RELAY_CONNECT_SCOPE]
}

fn valid_secret(value: &str) -> bool {
    !value.is_empty() && value.len() <= 16384 && !value.bytes().any(|b| b < 33 || b == 127)
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

struct RelayToken {
    token: Zeroizing<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectResponse {
    environment_id: EnvironmentId,
    endpoint: RelayEndpoint,
    credential: String,
    expires_at: String,
}

#[derive(Deserialize)]
struct RelayTokenResponse {
    access_token: String,
    issued_token_type: String,
    token_type: String,
    expires_in: f64,
    scope: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRelayToken {
    version: u32,
    origin: String,
    environment: EnvironmentId,
    principal: String,
    thumbprint: String,
    token: String,
    expires: u64,
    scope: String,
}

impl Drop for StoredRelayToken {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

impl Drop for ConnectResponse {
    fn drop(&mut self) {
        self.credential.zeroize();
    }
}

impl Drop for RelayTokenResponse {
    fn drop(&mut self) {
        self.access_token.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cloud::CloudConfig,
        credentials::MemoryCredentialStore,
        rpc::{ReadEvent, ReadSession},
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use futures_util::{SinkExt, StreamExt};
    use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
    use sha2::{Digest, Sha256};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::tungstenite::Message;

    async fn request(stream: &mut TcpStream) -> (String, String) {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut b = [0];
            stream.read_exact(&mut b).await.unwrap();
            head.push(b[0]);
        }
        let head = String::from_utf8(head).unwrap();
        let len = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        let mut body = vec![0; len];
        stream.read_exact(&mut body).await.unwrap();
        (head, String::from_utf8(body).unwrap())
    }

    async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
        stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    }

    fn header<'a>(head: &'a str, name: &str) -> &'a str {
        head.lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                key.eq_ignore_ascii_case(name).then_some(value.trim())
            })
            .unwrap()
    }

    fn verify(proof: &str, method: &str, url: &str, token: Option<&str>) -> String {
        let parts: Vec<_> = proof.split('.').collect();
        assert_eq!(parts.len(), 3);
        let decode = |s| URL_SAFE_NO_PAD.decode(s).unwrap();
        let h: serde_json::Value = serde_json::from_slice(&decode(parts[0])).unwrap();
        let p: serde_json::Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        assert_eq!(h["typ"], "dpop+jwt");
        assert_eq!(h["alg"], "ES256");
        assert!(h["jwk"].get("d").is_none());
        assert_eq!(p["htm"], method);
        assert_eq!(p["htu"], url);
        match token {
            Some(token) => assert_eq!(
                p["ath"],
                URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
            ),
            None => assert!(p.get("ath").is_none()),
        }
        let mut key = vec![4];
        key.extend(decode(h["jwk"]["x"].as_str().unwrap()));
        key.extend(decode(h["jwk"]["y"].as_str().unwrap()));
        VerifyingKey::from_sec1_bytes(&key)
            .unwrap()
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &Signature::from_slice(&decode(parts[2])).unwrap(),
            )
            .unwrap();
        let canonical = format!(
            "{{\"crv\":\"P-256\",\"kty\":\"EC\",\"x\":\"{}\",\"y\":\"{}\"}}",
            h["jwk"]["x"].as_str().unwrap(),
            h["jwk"]["y"].as_str().unwrap()
        );
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
    }

    #[tokio::test]
    async fn oauth_directory_exposes_pairable_routes_without_a_dpop_exchange() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let server_origin = origin.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (head, _) = request(&mut stream).await;
            assert!(head.starts_with("GET /v1/environments "));
            assert_eq!(header(&head, "authorization"), "Bearer cloud-fixture");
            assert!(!head
                .lines()
                .any(|line| line.to_ascii_lowercase().starts_with("dpop:")));
            respond(&mut stream, "200 OK", &serde_json::json!({"environments":[{
                "environmentId":"public-tunnel", "label":"Public tunnel", "linkedAt":"2026-01-01T00:00:00Z",
                "endpoint":{"httpBaseUrl":server_origin, "wsBaseUrl":server_origin.replace("http:","ws:"), "providerKind":"cloudflare_tunnel"}
            }]}).to_string()).await;
        });
        let entries = RelayClient::loopback_fixture(&origin)
            .unwrap()
            .list_environments(&CloudGrant::fixture())
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id.as_str(), "public-tunnel");
        assert!(
            matches!(&entries[0].routes[0], Route::Bearer { http_base_url: Some(url), .. } if url == &origin)
        );
        server.await.unwrap();
    }

    fn directory_record(id: &str, origin: &str) -> serde_json::Value {
        serde_json::json!({"environmentId":id, "label":"Remote", "endpoint":{
            "httpBaseUrl":origin, "wsBaseUrl":origin.replace("https:","wss:").replace("http:","ws:"),
            "providerKind":"cloudflare_tunnel"
        }})
    }

    #[test]
    fn directory_rejects_unsafe_routes_and_ambiguous_identities() {
        let project = |records: Vec<serde_json::Value>| {
            directory_entries(
                serde_json::from_value(serde_json::json!({"environments":records})).unwrap(),
                false,
            )
        };
        for origin in [
            "http://127.0.0.1:3773/",
            "https://user:secret@remote.example/",
            "https://remote.example/?token=secret",
            "https://remote.example/#secret",
            "https://remote.example/prefix/",
        ] {
            assert!(
                project(vec![directory_record("a", origin)]).is_err(),
                "unsafe origin was accepted"
            );
        }
        let mut mismatched = directory_record("a", "https://remote.example/");
        mismatched["endpoint"]["wsBaseUrl"] = "wss://other.example/".into();
        assert!(project(vec![mismatched]).is_err());
        let mut blank = directory_record("a", "https://remote.example/");
        blank["label"] = "  ".into();
        assert!(project(vec![blank]).is_err());
        assert!(project(vec![
            directory_record("a", "https://a.example/"),
            directory_record("a", "https://b.example/")
        ])
        .is_err());
        assert!(project(vec![
            directory_record("a", "https://a.example/"),
            directory_record("b", "https://a.example/")
        ])
        .is_err());
        assert_eq!(
            project(vec![directory_record("a", "https://remote.example/")])
                .unwrap()
                .len(),
            1
        );
        assert!(matches!(
            project(
                (0..129)
                    .map(|i| directory_record(
                        &format!("env-{i}"),
                        &format!("https://host-{i}.example/")
                    ))
                    .collect()
            ),
            Err(ConnectionError::Protocol)
        ));
    }

    #[tokio::test]
    async fn directory_rejection_does_not_attempt_a_dpop_exchange() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let cloud = CloudGrant::fixture();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (head, _) = request(&mut stream).await;
            assert!(head.starts_with("GET /v1/environments "));
            respond(&mut stream, "401 Unauthorized", "{}").await;
            assert!(
                tokio::time::timeout(Duration::from_millis(150), listener.accept())
                    .await
                    .is_err()
            );
        });
        assert!(matches!(
            RelayClient::loopback_fixture(&origin)
                .unwrap()
                .list_environments(&cloud)
                .await,
            Err(ConnectionError::PermissionDenied)
        ));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn relay_to_rpc_and_persisted_reload() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = format!("http://{addr}/");
        let server_origin = origin.clone();
        let server = tokio::spawn(async move {
            let descriptor = r#"{"environmentId":"env-a","label":"Test","platform":{"os":"linux","arch":"x64"},"serverVersion":"1","capabilities":{}}"#;
            for step in 0..15 {
                let (mut stream, _) = listener.accept().await.unwrap();
                if step == 8 {
                    let expected = server_origin.clone();
                    let mut socket = tokio_tungstenite::accept_hdr_async(stream, move |request: &tokio_tungstenite::tungstenite::handshake::server::Request, response| {
                        let url = Url::parse(&format!("http://fixture{}", request.uri())).unwrap();
                        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
                        assert_eq!(params["wsTicket"], "ticket-fixture");
                        verify(&params["dpop"], "GET", &format!("{}ws", expected), Some("ticket-fixture"));
                        Ok(response)
                    }).await.unwrap();
                    let msg = socket.next().await.unwrap().unwrap().into_text().unwrap();
                    let msg: serde_json::Value = serde_json::from_str(&msg).unwrap();
                    assert_eq!(msg["tag"], "server.getConfig");
                    socket.send(Message::Text(format!(r#"{{"_tag":"Exit","requestId":{},"exit":{{"_tag":"Success","value":{{"ok":true}}}}}}"#, msg["id"]).into())).await.unwrap();
                    let msg = socket.next().await.unwrap().unwrap().into_text().unwrap();
                    let msg: serde_json::Value = serde_json::from_str(&msg).unwrap();
                    assert_eq!(msg["tag"], "server.getUsageSummary");
                    socket.send(Message::Text(format!(r#"{{"_tag":"Exit","requestId":{},"exit":{{"_tag":"Failure","cause":[{{"_tag":"Fail","error":{{"_tag":"EnvironmentAuthorizationError"}}}}]}}}}"#, msg["id"]).into())).await.unwrap();
                    continue;
                }
                let (head, body) = request(&mut stream).await;
                match step {
                    0 => {
                        assert!(head.starts_with("POST /v1/client/dpop-token "));
                        verify(
                            header(&head, "dpop"),
                            "POST",
                            &format!("{}v1/client/dpop-token", server_origin),
                            None,
                        );
                        assert!(body.contains("subject_token=cloud-fixture"));
                        let form: std::collections::HashMap<_, _> =
                            url::form_urlencoded::parse(body.as_bytes())
                                .into_owned()
                                .collect();
                        assert_eq!(form["resource"], server_origin.trim_end_matches('/'));
                        respond(&mut stream, "200 OK", r#"{"access_token":"relay-fixture","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"DPoP","expires_in":3600,"scope":"environment:connect"}"#).await;
                    }
                    1 | 9 | 11 => {
                        assert!(head.starts_with("POST /v1/environments/env-a/connect "));
                        assert_eq!(header(&head, "authorization"), "DPoP relay-fixture");
                        let thumbprint = verify(
                            header(&head, "dpop"),
                            "POST",
                            &format!("{}v1/environments/env-a/connect", server_origin),
                            Some("relay-fixture"),
                        );
                        let payload: serde_json::Value = serde_json::from_str(&body).unwrap();
                        assert_eq!(payload["clientKeyThumbprint"], thumbprint);
                        respond(&mut stream, "200 OK", &format!(r#"{{"environmentId":"env-a","endpoint":{{"httpBaseUrl":"{server_origin}","wsBaseUrl":"ws://{addr}/","providerKind":"t3_relay"}},"credential":"bootstrap-fixture","expiresAt":"2099-01-01T00:00:00Z"}}"#)).await;
                    }
                    2 | 3 | 5 | 10 | 12 | 13 => {
                        assert!(head.starts_with("GET /.well-known/t3/environment "));
                        respond(&mut stream, "200 OK", descriptor).await;
                    }
                    4 | 14 => {
                        assert!(head.starts_with("POST /oauth/token "));
                        verify(
                            header(&head, "dpop"),
                            "POST",
                            &format!("{}oauth/token", server_origin),
                            None,
                        );
                        assert!(body.contains("subject_token=bootstrap-fixture"));
                        assert!(body.contains("scope=orchestration%3Aread+diagnostics%3Aread"));
                        respond(&mut stream, "200 OK", r#"{"access_token":"env-fixture","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"DPoP","expires_in":3600,"scope":"orchestration:read"}"#).await;
                    }
                    6 => {
                        assert_eq!(header(&head, "authorization"), "DPoP env-fixture");
                        verify(
                            header(&head, "dpop"),
                            "POST",
                            &format!("{}api/auth/websocket-ticket/rpc", server_origin),
                            Some("env-fixture"),
                        );
                        respond(&mut stream, "404 Not Found", "{}").await;
                    }
                    7 => {
                        assert_eq!(header(&head, "authorization"), "DPoP env-fixture");
                        verify(
                            header(&head, "dpop"),
                            "POST",
                            &format!("{}api/auth/websocket-ticket", server_origin),
                            Some("env-fixture"),
                        );
                        respond(
                            &mut stream,
                            "200 OK",
                            r#"{"ticket":"ticket-fixture","expiresAt":"2099-01-01T00:00:00Z"}"#,
                        )
                        .await;
                    }
                    _ => unreachable!(),
                }
            }
        });
        let store = MemoryCredentialStore::default();
        let cloud = CloudGrant::fixture();
        let signer = CloudConfig::loopback_fixture(&origin, &origin)
            .unwrap()
            .signer(&store)
            .unwrap();
        let relay = RelayClient::loopback_fixture(&origin).unwrap();
        let id = EnvironmentId::new("env-a").unwrap();
        let connection = relay.connect(&id, &cloud, &signer, &store).await.unwrap();
        assert!(connection.grant.orchestration_read);
        assert!(!connection.grant.diagnostics_read);
        let mut session = ReadSession::connect(&connection.endpoint, &connection.grant)
            .await
            .unwrap();
        session.get_config().await.unwrap();
        assert!(matches!(
            session.next_event().await.unwrap(),
            ReadEvent::Success { .. }
        ));
        session
            .get_usage_summary("2026-01-01", "2026-01-02", "UTC", "day", None, None, None)
            .await
            .unwrap();
        assert!(matches!(
            session.next_event().await.unwrap(),
            ReadEvent::PermissionDenied { .. }
        ));
        let signer = CloudConfig::loopback_fixture(&origin, &origin)
            .unwrap()
            .signer(&store)
            .unwrap();
        let mut restored = relay.connect(&id, &cloud, &signer, &store).await.unwrap();
        assert!(restored.grant.orchestration_read);
        assert!(!restored.grant.diagnostics_read);
        restored.grant.expires_wall = SystemTime::now() - Duration::from_secs(1);
        let scope = CredentialScope::relay_environment_grant(
            restored.endpoint.base.as_str(),
            &id,
            cloud.cache_key(),
            &signer.thumbprint(),
        );
        restored
            .endpoint
            .save_grant_at_scope(&restored.grant, &store, &scope)
            .unwrap();
        let renewed = relay.connect(&id, &cloud, &signer, &store).await.unwrap();
        assert!(renewed.grant.orchestration_read);
        assert!(!renewed.grant.diagnostics_read);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_wrong_descriptor_before_bootstrap() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = format!("http://{addr}/");
        let endpoint = origin.clone();
        let server = tokio::spawn(async move {
            for step in 0..3 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (head, _) = request(&mut stream).await;
                match step {
                    0 => respond(&mut stream, "200 OK", r#"{"access_token":"relay-fixture","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"DPoP","expires_in":3600,"scope":"environment:connect"}"#).await,
                    1 => respond(&mut stream, "200 OK", &format!(r#"{{"environmentId":"env-a","endpoint":{{"httpBaseUrl":"{endpoint}","wsBaseUrl":"ws://{addr}/","providerKind":"manual"}},"credential":"bootstrap-fixture","expiresAt":"2099-01-01T00:00:00Z"}}"#)).await,
                    2 => {
                        assert!(head.starts_with("GET /.well-known/t3/environment "));
                        assert!(!head.contains("bootstrap-fixture"));
                        respond(&mut stream, "200 OK", r#"{"environmentId":"wrong","label":"Test","platform":{"os":"linux","arch":"x64"},"serverVersion":"1","capabilities":{}}"#).await;
                    }
                    _ => unreachable!(),
                }
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(150), listener.accept())
                    .await
                    .is_err()
            );
        });
        let store = MemoryCredentialStore::default();
        let signer = CloudConfig::loopback_fixture(&origin, &origin)
            .unwrap()
            .signer(&store)
            .unwrap();
        let result = RelayClient::loopback_fixture(&origin)
            .unwrap()
            .connect(
                &EnvironmentId::new("env-a").unwrap(),
                &CloudGrant::fixture(),
                &signer,
                &store,
            )
            .await;
        assert!(matches!(result, Err(ConnectionError::WrongEnvironment)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_widened_relay_scope_before_connect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            request(&mut stream).await;
            respond(&mut stream, "200 OK", r#"{"access_token":"relay-fixture","issued_token_type":"urn:ietf:params:oauth:token-type:access_token","token_type":"DPoP","expires_in":3600,"scope":"environment:connect environment:status"}"#).await;
            assert!(
                tokio::time::timeout(Duration::from_millis(150), listener.accept())
                    .await
                    .is_err()
            );
        });
        let store = MemoryCredentialStore::default();
        let signer = CloudConfig::loopback_fixture(&origin, &origin)
            .unwrap()
            .signer(&store)
            .unwrap();
        let result = RelayClient::loopback_fixture(&origin)
            .unwrap()
            .connect(
                &EnvironmentId::new("env-a").unwrap(),
                &CloudGrant::fixture(),
                &signer,
                &store,
            )
            .await;
        assert!(matches!(result, Err(ConnectionError::Protocol)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejected_oauth_subject_is_unsupported_without_removing_saved_access() {
        struct ReadOnlyStore;
        impl CredentialStore for ReadOnlyStore {
            fn load(&self, _: &CredentialScope) -> Result<Option<Zeroizing<Vec<u8>>>, ConnectionError> { Ok(None) }
            fn save(&self, _: &CredentialScope, _: &[u8]) -> Result<(), ConnectionError> { panic!("rejected exchange must not save") }
            fn delete(&self, _: &CredentialScope) -> Result<(), ConnectionError> { panic!("rejected exchange must not delete") }
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (head, _) = request(&mut stream).await;
            assert!(head.starts_with("POST /v1/client/dpop-token "));
            respond(&mut stream, "401 Unauthorized", r#"{"_tag":"RelayAuthInvalidError","code":"auth_invalid","reason":"invalid_bearer"}"#).await;
        });
        let signer = CloudConfig::loopback_fixture(&origin, &origin).unwrap()
            .signer(&MemoryCredentialStore::default()).unwrap();
        let result = RelayClient::loopback_fixture(&origin).unwrap().connect(
            &EnvironmentId::new("env-a").unwrap(), &CloudGrant::fixture(), &signer, &ReadOnlyStore,
        ).await;
        assert!(matches!(result, Err(ConnectionError::RelayCredentialUnsupported)));
        server.await.unwrap();
    }

    #[test]
    fn rejects_socket_origin_mismatch() {
        let endpoint = RelayEndpoint {
            http_base_url: "https://environment.example/".into(),
            ws_base_url: "wss://other.example/ws".into(),
            provider_kind: "t3_relay".into(),
        };
        assert_eq!(
            validate_endpoint(&endpoint),
            Err(ConnectionError::InvalidDescriptor)
        );
        let downgraded = RelayEndpoint {
            http_base_url: "https://environment.example/".into(),
            ws_base_url: "ws://environment.example:443/ws".into(),
            provider_kind: "manual".into(),
        };
        assert_eq!(
            validate_endpoint(&downgraded),
            Err(ConnectionError::InvalidDescriptor)
        );
    }
}
