use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use rand::{distributions::Alphanumeric, Rng};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

const RELAY_URL: &str = "https://relay.t3.codes";
const HOSTED_CONNECT_URL: &str = "https://app.t3.codes/connect";
const CLERK_FRONTEND_API: &str = "https://clerk.t3.codes";
const CLERK_CLIENT_ID: &str = "hzxSgY2cH10sDU2r";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayEnvironment {
    pub environment_id: String,
    pub label: String,
    pub endpoint: RelayEndpoint,
    pub linked_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayEndpoint {
    pub http_base_url: String,
    pub ws_base_url: String,
    pub provider_kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectResponse {
    #[serde(rename = "environmentId")]
    pub environment_id: String,
    pub endpoint: RelayEndpoint,
    pub credential: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    #[serde(rename = "environmentId")]
    pub environment_id: String,
    pub endpoint: RelayEndpoint,
    pub status: String,
    #[serde(default)]
    pub descriptor: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct DpopTokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Clone)]
pub struct DpopSigner {
    key: SigningKey,
    thumbprint: String,
}

impl DpopSigner {
    pub fn generate() -> Self {
        let key = SigningKey::random(&mut rand::thread_rng());
        let point = key.verifying_key().to_encoded_point(false);
        let x = URL_SAFE_NO_PAD.encode(point.x().expect("p256 x coordinate"));
        let y = URL_SAFE_NO_PAD.encode(point.y().expect("p256 y coordinate"));
        let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
        let thumbprint = URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()));
        Self { key, thumbprint }
    }

    pub fn thumbprint(&self) -> &str { &self.thumbprint }

    pub fn key_bytes(&self) -> [u8; 32] { self.key.to_bytes().into() }

    pub fn from_key_bytes(bytes: &[u8]) -> Result<Self, String> {
        let key = SigningKey::from_slice(bytes).map_err(|_| "stored relay signing key was invalid")?;
        let point = key.verifying_key().to_encoded_point(false);
        let x = URL_SAFE_NO_PAD.encode(point.x().ok_or("missing relay signing key x coordinate")?);
        let y = URL_SAFE_NO_PAD.encode(point.y().ok_or("missing relay signing key y coordinate")?);
        let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
        let thumbprint = URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()));
        Ok(Self { key, thumbprint })
    }

    pub fn proof(&self, method: &str, target: &str) -> Result<String, String> {
        let point = self.key.verifying_key().to_encoded_point(false);
        let header = format!(
            r#"{{"alg":"ES256","jwk":{{"crv":"P-256","kty":"EC","x":"{}","y":"{}"}},"typ":"dpop+jwt"}}"#,
            URL_SAFE_NO_PAD.encode(point.x().ok_or("missing x")?),
            URL_SAFE_NO_PAD.encode(point.y().ok_or("missing y")?),
        );
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "clock error")?.as_secs();
        let jti: String = rand::thread_rng().sample_iter(&Alphanumeric).take(32).map(char::from).collect();
        let payload = format!(r#"{{"htu":"{target}","htm":"{method}","iat":{now},"jti":"{jti}"}}"#);
        let signing_input = format!("{}.{}", URL_SAFE_NO_PAD.encode(header), URL_SAFE_NO_PAD.encode(payload));
        let signature: Signature = self.key.sign(signing_input.as_bytes());
        let raw = signature.to_bytes();
        Ok(format!("{}.{}", signing_input, URL_SAFE_NO_PAD.encode(raw)))
    }
}

#[derive(Clone)]
pub struct RelayClient {
    relay_url: String,
    http: Client,
    signer: DpopSigner,
}

#[derive(Debug, Clone)]
pub struct PkceRequest {
    pub verifier: String,
    pub challenge: String,
    pub state: String,
}

pub fn make_pkce_request() -> PkceRequest {
    let verifier: String = rand::thread_rng().sample_iter(&Alphanumeric).take(64).map(char::from).collect();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state: String = rand::thread_rng().sample_iter(&Alphanumeric).take(32).map(char::from).collect();
    PkceRequest { verifier, challenge, state }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

impl RelayClient {
    pub fn new() -> Result<Self, String> {
        Self::with_url(RELAY_URL)
    }

    pub fn with_url(relay_url: &str) -> Result<Self, String> {
        let url = Url::parse(relay_url).map_err(|_| "invalid relay URL")?;
        if url.scheme() != "https" || url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err("relay URL must be a secure HTTPS origin".into());
        }
        Ok(Self { relay_url: url.origin().ascii_serialization(), http: Client::new(), signer: DpopSigner::generate() })
    }

    pub fn signer(&self) -> &DpopSigner { &self.signer }
    pub fn signing_key_bytes(&self) -> [u8; 32] { self.signer.key_bytes() }

    pub fn with_signing_key(relay_url: &str, key: &[u8]) -> Result<Self, String> {
        let url = Url::parse(relay_url).map_err(|_| "invalid relay URL")?;
        if url.scheme() != "https" || url.path() != "/" || url.query().is_some() || url.fragment().is_some() { return Err("relay URL must be a secure HTTPS origin".into()); }
        Ok(Self { relay_url: url.origin().ascii_serialization(), http: Client::new(), signer: DpopSigner::from_key_bytes(key)? })
    }

    pub fn authorization_url(state: &str, challenge: &str, port: u16) -> String {
        format!("{HOSTED_CONNECT_URL}#state={state}&challenge={challenge}&port={port}")
    }

    pub async fn exchange_oauth_code(&self, code: &str, verifier: &str, port: u16) -> Result<OAuthTokenResponse, String> {
        let response = self.http.post(format!("{CLERK_FRONTEND_API}/oauth/token")).form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &format!("http://127.0.0.1:{port}/callback")),
            ("client_id", CLERK_CLIENT_ID),
            ("code_verifier", verifier),
        ]).send().await.map_err(|_| "cloud login token exchange failed")?;
        if !response.status().is_success() { return Err(format!("cloud login token exchange returned {}", response.status())); }
        response.json().await.map_err(|_| "cloud login token response was invalid".into())
    }

    pub async fn refresh_oauth_token(&self, refresh_token: &str) -> Result<OAuthTokenResponse, String> {
        let response = self.http.post(format!("{CLERK_FRONTEND_API}/oauth/token")).form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLERK_CLIENT_ID),
        ]).send().await.map_err(|_| "cloud login refresh failed")?;
        if !response.status().is_success() { return Err(format!("cloud login refresh returned {}", response.status())); }
        response.json().await.map_err(|_| "cloud refresh response was invalid".into())
    }

    pub async fn list_environments(&self, clerk_token: &str) -> Result<Vec<RelayEnvironment>, String> {
        let response = self.http.get(format!("{}/v1/environments", self.relay_url))
            .bearer_auth(clerk_token).send().await.map_err(|_| "relay environment listing failed")?;
        if !response.status().is_success() { return Err(format!("relay environment listing returned {}", response.status())); }
        let body: serde_json::Value = response.json().await.map_err(|_| "relay environment listing was invalid")?;
        serde_json::from_value(body.get("environments").cloned().ok_or("relay response omitted environments")?)
            .map_err(|_| "relay environment record was invalid".into())
    }

    pub async fn exchange_dpop(&self, clerk_token: &str) -> Result<(String, u64), String> {
        let target = format!("{}/v1/client/dpop-token", self.relay_url);
        let proof = self.signer.proof("POST", &target)?;
        let response = self.http.post(&target).header("dpop", proof).form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:token-exchange"),
            ("subject_token", clerk_token),
            ("subject_token_type", "urn:ietf:params:oauth:token-type:jwt"),
            ("requested_token_type", "urn:ietf:params:oauth:token-type:access_token"),
            ("resource", self.relay_url.as_str()),
            ("scope", "environment:status environment:connect"),
            ("client_id", "t3-web"),
        ]).send().await.map_err(|_| "relay token exchange failed")?;
        if !response.status().is_success() { return Err(format!("relay token exchange returned {}", response.status())); }
        let body: DpopTokenResponse = response.json().await.map_err(|_| "relay token response was invalid")?;
        Ok((body.access_token, body.expires_in))
    }

    pub async fn status(&self, dpop_token: &str, environment_id: &str) -> Result<StatusResponse, String> {
        let target = format!("{}/v1/environments/{environment_id}/status", self.relay_url);
        let proof = self.signer.proof("POST", &target)?;
        let response = self.http.post(&target).header("authorization", format!("DPoP {dpop_token}")).header("dpop", proof).send().await.map_err(|_| "relay status request failed")?;
        if !response.status().is_success() { return Err(format!("relay status returned {}", response.status())); }
        response.json().await.map_err(|_| "relay status response was invalid".into())
    }

    pub async fn connect(&self, dpop_token: &str, environment_id: &str) -> Result<ConnectResponse, String> {
        let target = format!("{}/v1/environments/{environment_id}/connect", self.relay_url);
        let proof = self.signer.proof("POST", &target)?;
        let response = self.http.post(&target).header("authorization", format!("DPoP {dpop_token}")).header("dpop", proof).json(&serde_json::json!({"clientProofKeyThumbprint": self.signer.thumbprint()})).send().await.map_err(|_| "relay connect request failed")?;
        if !response.status().is_success() { return Err(format!("relay connect returned {}", response.status())); }
        response.json().await.map_err(|_| "relay connect response was invalid".into())
    }

    pub async fn shell_snapshot(&self, endpoint: &RelayEndpoint, credential: &str) -> Result<Vec<u8>, String> {
        let target = format!("{}/api/orchestration/shell", endpoint.http_base_url.trim_end_matches('/'));
        let proof = self.signer.proof("GET", &target)?;
        let response = self.http.get(&target)
            .header("authorization", format!("DPoP {credential}"))
            .header("dpop", proof)
            .header("x-t3-orchestration-protocol", "2")
            .send().await.map_err(|_| "remote shell request failed")?;
        if !response.status().is_success() { return Err(format!("remote shell returned {}", response.status())); }
        Ok(response.bytes().await.map_err(|_| "remote shell response was invalid")?.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpop_proof_has_jwt_shape_and_public_key_thumbprint() {
        let signer = DpopSigner::generate();
        let proof = signer.proof("POST", "https://relay.example/v1/client/dpop-token").unwrap();
        assert_eq!(proof.split('.').count(), 3);
        assert!(!signer.thumbprint().is_empty());
    }

    #[test]
    fn relay_url_rejects_non_https_origins() {
        assert!(RelayClient::with_url("http://relay.example").is_err());
        assert!(RelayClient::with_url("https://relay.example/path").is_err());
    }
}
