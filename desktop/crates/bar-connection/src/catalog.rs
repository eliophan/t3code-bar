use aes::Aes128;
use bar_identity::EnvironmentId;
use base64::{engine::general_purpose::STANDARD, Engine};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentMetadata {
    pub id: EnvironmentId,
    pub label: String,
    pub enabled: bool,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    Primary {
        http_base_url: String,
    },
    Bearer {
        connection_id: String,
        http_base_url: Option<String>,
    },
    Relay,
    Ssh {
        connection_id: String,
        target: Option<SshTarget>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshTarget {
    pub alias: String,
    pub hostname: String,
    pub username: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CatalogError {
    Io,
    KeyUnavailable,
    UnsupportedEncryption,
    InvalidCatalog,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    version: u32,
    encrypted_catalog: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    schema_version: u32,
    targets: Vec<Target>,
    profiles: Vec<Profile>,
    #[serde(default)]
    disabled_environment_ids: Vec<EnvironmentId>,
}
#[derive(Deserialize)]
#[serde(tag = "_tag")]
enum Target {
    BearerConnectionTarget {
        #[serde(rename = "environmentId")]
        environment_id: EnvironmentId,
        label: String,
        #[serde(rename = "connectionId")]
        connection_id: String,
    },
    RelayConnectionTarget {
        #[serde(rename = "environmentId")]
        environment_id: EnvironmentId,
        label: String,
    },
    SshConnectionTarget {
        #[serde(rename = "environmentId")]
        environment_id: EnvironmentId,
        label: String,
        #[serde(rename = "connectionId")]
        connection_id: String,
    },
}
#[derive(Deserialize)]
#[serde(tag = "_tag")]
enum Profile {
    BearerConnectionProfile {
        #[serde(rename = "connectionId")]
        connection_id: String,
        #[serde(rename = "environmentId")]
        environment_id: EnvironmentId,
        #[serde(rename = "httpBaseUrl")]
        http_base_url: String,
    },
    SshConnectionProfile {
        #[serde(rename = "connectionId")]
        connection_id: String,
        #[serde(rename = "environmentId")]
        environment_id: EnvironmentId,
        target: SshTarget,
    },
}

/// Read the Linux Chromium safeStorage catalog. Only metadata escapes this function.
pub fn discover_linux(
    path: &Path,
    primary: EnvironmentMetadata,
) -> Result<Vec<EnvironmentMetadata>, CatalogError> {
    let envelope = fs::read(path).map_err(|_| CatalogError::Io)?;
    let mut child = Command::new("/usr/bin/secret-tool")
        .args(["lookup", "application", "t3code"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CatalogError::KeyUnavailable)?;
    let stdout = child.stdout.take().ok_or(CatalogError::KeyUnavailable)?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Zeroizing::new(Vec::new());
        let result = stdout.take(16385).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let mut password = receiver
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| CatalogError::KeyUnavailable)?
        .map_err(|_| CatalogError::KeyUnavailable)?;
    if !status.is_some_and(|s| s.success()) || password.len() > 16384 {
        return Err(CatalogError::KeyUnavailable);
    }
    if password.last() == Some(&b'\n') {
        password.pop();
    }
    if password.is_empty() {
        return Err(CatalogError::KeyUnavailable);
    }
    decrypt_and_project(&envelope, &password, primary)
}

/// Decrypt a v11 safeStorage envelope using its keyring password and project only route metadata.
pub fn decrypt_and_project(
    envelope: &[u8],
    password: &[u8],
    primary: EnvironmentMetadata,
) -> Result<Vec<EnvironmentMetadata>, CatalogError> {
    let envelope: Envelope =
        serde_json::from_slice(envelope).map_err(|_| CatalogError::InvalidCatalog)?;
    if envelope.version != 1 {
        return Err(CatalogError::InvalidCatalog);
    }
    let ciphertext = Zeroizing::new(
        STANDARD
            .decode(envelope.encrypted_catalog)
            .map_err(|_| CatalogError::InvalidCatalog)?,
    );
    if !ciphertext.starts_with(b"v11") {
        return Err(CatalogError::UnsupportedEncryption);
    }
    let mut key = Zeroizing::new([0u8; 16]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", 1, &mut *key);
    let mut plaintext = Zeroizing::new(ciphertext[3..].to_vec());
    let bytes = cbc::Decryptor::<Aes128>::new(&(*key).into(), &[b' '; 16].into())
        .decrypt_padded_mut::<Pkcs7>(&mut plaintext)
        .map_err(|_| CatalogError::InvalidCatalog)?;
    let document: Document =
        serde_json::from_slice(bytes).map_err(|_| CatalogError::InvalidCatalog)?;
    if document.schema_version != 1 {
        return Err(CatalogError::InvalidCatalog);
    }
    for route in &primary.routes {
        match route {
            Route::Primary { http_base_url } => {
                super::validated_base(http_base_url).map_err(|_| CatalogError::InvalidCatalog)?;
            }
            Route::Bearer {
                http_base_url: Some(url),
                ..
            } => {
                super::validated_base(url).map_err(|_| CatalogError::InvalidCatalog)?;
            }
            _ => {}
        }
    }
    let disabled: BTreeSet<_> = document.disabled_environment_ids.into_iter().collect();
    let mut entries = BTreeMap::new();
    entries.insert(primary.id.clone(), primary);
    for target in document.targets {
        let (id, label, route) = match target {
            Target::BearerConnectionTarget {
                environment_id,
                label,
                connection_id,
            } => {
                let url = document.profiles.iter().find_map(|p| match p {
                    Profile::BearerConnectionProfile {
                        connection_id: c,
                        environment_id: e,
                        http_base_url,
                    } if c == &connection_id && e == &environment_id => {
                        super::validated_base(http_base_url)
                            .ok()
                            .map(|u| u.to_string())
                    }
                    _ => None,
                });
                (
                    environment_id,
                    label,
                    Route::Bearer {
                        connection_id,
                        http_base_url: url,
                    },
                )
            }
            Target::RelayConnectionTarget {
                environment_id,
                label,
            } => (environment_id, label, Route::Relay),
            Target::SshConnectionTarget {
                environment_id,
                label,
                connection_id,
            } => {
                let target = document.profiles.iter().find_map(|p| match p {
                    Profile::SshConnectionProfile {
                        connection_id: c,
                        environment_id: e,
                        target,
                    } if c == &connection_id && e == &environment_id => Some(target.clone()),
                    _ => None,
                });
                (
                    environment_id,
                    label,
                    Route::Ssh {
                        connection_id,
                        target,
                    },
                )
            }
        };
        let entry = entries
            .entry(id.clone())
            .or_insert_with(|| EnvironmentMetadata {
                id: id.clone(),
                label,
                enabled: !disabled.contains(&id),
                routes: Vec::new(),
            });
        if !entry.routes.contains(&route) {
            entry.routes.push(route);
        }
    }
    Ok(entries.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;
    fn primary() -> EnvironmentMetadata {
        EnvironmentMetadata {
            id: EnvironmentId::new("local").unwrap(),
            label: "Local".into(),
            enabled: true,
            routes: vec![Route::Primary {
                http_base_url: "http://127.0.0.1:3000/".into(),
            }],
        }
    }
    fn envelope(json: &str) -> Vec<u8> {
        let mut key = [0u8; 16];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(b"synthetic", b"saltysalt", 1, &mut key);
        let mut bytes = b"v11".to_vec();
        bytes.extend(
            cbc::Encryptor::<Aes128>::new(&key.into(), &[b' '; 16].into())
                .encrypt_padded_vec_mut::<Pkcs7>(json.as_bytes()),
        );
        serde_json::to_vec(
            &serde_json::json!({"version": 1, "encryptedCatalog": STANDARD.encode(bytes)}),
        )
        .unwrap()
    }
    #[test]
    fn decrypts_and_projects_routes_without_credentials() {
        let encrypted = envelope(
            r#"{"schemaVersion":1,"targets":[{"_tag":"RelayConnectionTarget","environmentId":"remote","label":"Remote"},{"_tag":"BearerConnectionTarget","environmentId":"remote","label":"Remote","connectionId":"direct"},{"_tag":"SshConnectionTarget","environmentId":"remote","label":"Remote","connectionId":"ssh"}],"profiles":[{"_tag":"BearerConnectionProfile","connectionId":"direct","environmentId":"remote","httpBaseUrl":"https://example.test/","wsBaseUrl":"wss://example.test/"},{"_tag":"SshConnectionProfile","connectionId":"ssh","environmentId":"remote","target":{"alias":"box","hostname":"host","username":null,"port":22}}],"credentials":[{"credential":{"token":"synthetic-secret"}}],"remoteDpopTokens":[],"disabledEnvironmentIds":["remote"]}"#,
        );
        let result = decrypt_and_project(&encrypted, b"synthetic", primary()).unwrap();
        assert_eq!(result.len(), 2);
        let mut offline_primary = primary();
        offline_primary.routes.clear();
        let offline = decrypt_and_project(&encrypted, b"synthetic", offline_primary).unwrap();
        assert_eq!(offline.len(), 2);
        assert!(offline
            .iter()
            .find(|e| e.id.as_str() == "local")
            .unwrap()
            .routes
            .is_empty());
        let remote = result.iter().find(|e| e.id.as_str() == "remote").unwrap();
        assert!(!remote.enabled);
        assert_eq!(remote.routes.len(), 3);
        assert!(
            matches!(&remote.routes[2], Route::Ssh { target: Some(target), .. } if target.hostname == "host")
        );
        assert!(!format!("{result:?}").contains("synthetic-secret"));
        assert_eq!(
            decrypt_and_project(&encrypted, b"wrong", primary()),
            Err(CatalogError::InvalidCatalog)
        );
    }
    #[test]
    fn rejects_sensitive_and_unsafe_catalog_routes() {
        for url in [
            "https://user:secret@example.test/",
            "https://example.test/?token=secret",
            "https://example.test/#secret",
            "http://example.test/",
            "https://example.test/path",
        ] {
            let document = serde_json::json!({
                "schemaVersion": 1,
                "targets": [{"_tag": "BearerConnectionTarget", "environmentId": "remote", "label": "Remote", "connectionId": "direct"}],
                "profiles": [{"_tag": "BearerConnectionProfile", "connectionId": "direct", "environmentId": "remote", "httpBaseUrl": url}]
            });
            let result =
                decrypt_and_project(&envelope(&document.to_string()), b"synthetic", primary())
                    .unwrap();
            let serialized = serde_json::to_string(&result).unwrap();
            assert!(!serialized.contains("secret"));
            assert!(matches!(
                &result[1].routes[0],
                Route::Bearer {
                    http_base_url: None,
                    ..
                }
            ));
        }
        let mut unsafe_primary = primary();
        unsafe_primary.routes = vec![Route::Primary {
            http_base_url: "https://user:secret@example.test/".into(),
        }];
        let document = envelope(r#"{"schemaVersion":1,"targets":[],"profiles":[]}"#);
        assert_eq!(
            decrypt_and_project(&document, b"synthetic", unsafe_primary),
            Err(CatalogError::InvalidCatalog)
        );
    }
    #[test]
    fn empty_catalog_is_success_and_bad_version_fails() {
        let encrypted = envelope(
            r#"{"schemaVersion":1,"targets":[],"profiles":[],"credentials":[],"remoteDpopTokens":[]}"#,
        );
        assert_eq!(
            decrypt_and_project(&encrypted, b"synthetic", primary())
                .unwrap()
                .len(),
            1
        );
        let bad = envelope(r#"{"schemaVersion":2,"targets":[],"profiles":[]}"#);
        assert_eq!(
            decrypt_and_project(&bad, b"synthetic", primary()),
            Err(CatalogError::InvalidCatalog)
        );
    }

    #[test]
    fn remote_catalog_survives_when_local_runtime_has_no_known_url() {
        let encrypted = envelope(
            r#"{"schemaVersion":1,"targets":[{"_tag":"RelayConnectionTarget","environmentId":"remote","label":"Remote"}],"profiles":[]}"#,
        );
        let local_without_runtime = EnvironmentMetadata {
            id: EnvironmentId::new("local").unwrap(),
            label: "Local".into(),
            enabled: true,
            routes: vec![],
        };
        let result = decrypt_and_project(&encrypted, b"synthetic", local_without_runtime).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].routes.is_empty());
        assert!(matches!(result[1].routes.as_slice(), [Route::Relay]));
    }
}
