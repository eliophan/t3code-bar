use crate::{
    credentials::{CloudPurpose, CredentialScope, CredentialStore},
    ConnectionError,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use p256::ecdsa::{signature::Signer, Signature, SigningKey};
use rand_core::{OsRng, RngCore};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;
use zeroize::Zeroizing;

/// A Bar-owned, persistent P-256 proof key. Its private material never leaves the store.
#[derive(Clone)]
pub struct DpopSigner(SigningKey);

impl fmt::Debug for DpopSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DpopSigner([redacted])")
    }
}

impl DpopSigner {
    pub(crate) fn load_or_create(
        store: &dyn CredentialStore,
        origin: &str,
    ) -> Result<Self, ConnectionError> {
        let scope = CredentialScope::cloud(origin, CloudPurpose::Signer);
        if let Some(bytes) = store.load(&scope)? {
            return SigningKey::from_slice(&bytes)
                .map(Self)
                .map_err(|_| ConnectionError::Storage);
        }
        let key = SigningKey::random(&mut OsRng);
        let bytes = Zeroizing::new(key.to_bytes());
        store.save(&scope, bytes.as_slice())?;
        Ok(Self(key))
    }
    fn coordinates(&self) -> (String, String) {
        let point = self.0.verifying_key().to_encoded_point(false);
        (
            URL_SAFE_NO_PAD.encode(point.x().unwrap()),
            URL_SAFE_NO_PAD.encode(point.y().unwrap()),
        )
    }
    pub fn thumbprint(&self) -> String {
        let (x, y) = self.coordinates();
        let canonical = format!("{{\"crv\":\"P-256\",\"kty\":\"EC\",\"x\":\"{x}\",\"y\":\"{y}\"}}");
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes()))
    }
    pub(crate) fn proof(
        &self,
        method: &str,
        url: &str,
        token: Option<&str>,
    ) -> Result<String, ConnectionError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ConnectionError::Protocol)?
            .as_secs();
        self.proof_at(method, url, token, now)
    }
    fn proof_at(
        &self,
        method: &str,
        url: &str,
        token: Option<&str>,
        now: u64,
    ) -> Result<String, ConnectionError> {
        if method.is_empty() || !method.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(ConnectionError::Protocol);
        }
        let mut url = Url::parse(url).map_err(|_| ConnectionError::InvalidRoute)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(ConnectionError::InvalidRoute);
        }
        url.set_query(None);
        url.set_fragment(None);
        let (x, y) = self.coordinates();
        let header =
            json!({"typ":"dpop+jwt","alg":"ES256","jwk":{"kty":"EC","crv":"P-256","x":x,"y":y}});
        let mut random = [0u8; 16];
        OsRng.fill_bytes(&mut random);
        let mut payload = json!({"htm":method.to_ascii_uppercase(),"htu":url.as_str(),"iat":now,"jti":URL_SAFE_NO_PAD.encode(random)});
        if let Some(token) = token {
            payload["ath"] = json!(URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes())));
        }
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&header).map_err(|_| ConnectionError::Protocol)?),
            URL_SAFE_NO_PAD
                .encode(serde_json::to_vec(&payload).map_err(|_| ConnectionError::Protocol)?)
        );
        let signature: Signature = self.0.sign(input.as_bytes());
        let signature = signature.normalize_s().unwrap_or(signature);
        Ok(format!(
            "{}.{}",
            input,
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryCredentialStore;
    use p256::ecdsa::{signature::Verifier, VerifyingKey};
    fn verify(proof: &str, method: &str, url: &str, token: &str, now: u64) -> bool {
        let parts: Vec<_> = proof.split('.').collect();
        let decode = |s| URL_SAFE_NO_PAD.decode(s).unwrap();
        let h: serde_json::Value = serde_json::from_slice(&decode(parts[0])).unwrap();
        let p: serde_json::Value = serde_json::from_slice(&decode(parts[1])).unwrap();
        let mut key = vec![4];
        key.extend(decode(h["jwk"]["x"].as_str().unwrap()));
        key.extend(decode(h["jwk"]["y"].as_str().unwrap()));
        let vk = VerifyingKey::from_sec1_bytes(&key).unwrap();
        let sig = Signature::from_slice(&decode(parts[2])).unwrap();
        let mut u = Url::parse(url).unwrap();
        u.set_query(None);
        u.set_fragment(None);
        h["typ"] == "dpop+jwt"
            && h["alg"] == "ES256"
            && h["jwk"].get("d").is_none()
            && p["htm"] == method
            && p["htu"] == u.as_str()
            && p["ath"] == URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
            && now.abs_diff(p["iat"].as_u64().unwrap()) <= 300
            && vk
                .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
                .is_ok()
    }
    #[test]
    fn persistent_real_signatures_and_binding() {
        let store = MemoryCredentialStore::default();
        let s = DpopSigner::load_or_create(&store, "https://clerk.t3.codes/").unwrap();
        let t = s.thumbprint();
        assert_eq!(
            t,
            DpopSigner::load_or_create(&store, "https://clerk.t3.codes/")
                .unwrap()
                .thumbprint()
        );
        let proof = s
            .proof_at(
                "post",
                "https://example.com/token?q=1#x",
                Some("secret"),
                1000,
            )
            .unwrap();
        assert!(verify(
            &proof,
            "POST",
            "https://example.com/token",
            "secret",
            1000
        ));
        assert!(!verify(
            &proof,
            "GET",
            "https://example.com/token",
            "secret",
            1000
        ));
        assert!(!verify(
            &proof,
            "POST",
            "https://example.com/other",
            "secret",
            1000
        ));
        assert!(!verify(
            &proof,
            "POST",
            "https://example.com/token",
            "wrong",
            1000
        ));
        assert!(!verify(
            &proof,
            "POST",
            "https://example.com/token",
            "secret",
            1301
        ));
        assert!(!format!("{s:?}").contains(&proof));
    }
}
