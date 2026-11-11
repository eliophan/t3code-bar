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

#[derive(Clone, Copy)]
enum SafeStorage {
    #[cfg(any(target_os = "linux", test))]
    Linux,
    #[cfg(any(target_os = "macos", test))]
    MacOs,
}

impl SafeStorage {
    fn parameters(self) -> (&'static [u8; 3], u32) {
        match self {
            #[cfg(any(target_os = "linux", test))]
            Self::Linux => (b"v11", 1),
            #[cfg(any(target_os = "macos", test))]
            Self::MacOs => (b"v10", 1003),
        }
    }
}

/// Read the desktop safeStorage catalog. Only route metadata escapes this boundary.
pub fn discover(
    path: &Path,
    primary: EnvironmentMetadata,
) -> Result<Vec<EnvironmentMetadata>, CatalogError> {
    let envelope = fs::read(path).map_err(|_| CatalogError::Io)?;
    #[cfg(target_os = "linux")]
    {
        let password = read_password(
            "/usr/bin/secret-tool",
            &["lookup", "application", "t3code"],
            Duration::from_secs(5),
        )?;
        decrypt_and_project(&envelope, &password, primary, SafeStorage::Linux)
    }
    #[cfg(target_os = "macos")]
    {
        discover_macos(&envelope, primary)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (envelope, primary);
        Err(CatalogError::UnsupportedEncryption)
    }
}

#[cfg(target_os = "macos")]
fn discover_macos(
    envelope: &[u8],
    primary: EnvironmentMetadata,
) -> Result<Vec<EnvironmentMetadata>, CatalogError> {
    use std::sync::{Mutex, OnceLock};
    struct KeyCache {
        password: Option<Zeroizing<Vec<u8>>>,
        retry_after: Instant,
    }
    static CACHE: OnceLock<Mutex<KeyCache>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| {
            Mutex::new(KeyCache {
                password: None,
                retry_after: Instant::now(),
            })
        })
        .lock()
        .map_err(|_| CatalogError::KeyUnavailable)?;
    if cache.password.is_none() {
        if Instant::now() < cache.retry_after {
            return Err(CatalogError::KeyUnavailable);
        }
        // Discovery runs on the refresh worker, never the AppKit thread. The child
        // is bounded and reaped; denying a prompt backs off instead of re-prompting.
        let password = read_password(
            "/usr/bin/security",
            &[
                "find-generic-password",
                "-w",
                "-s",
                "t3code Safe Storage",
                "-a",
                "t3code Key",
            ],
            Duration::from_secs(30),
        );
        cache.retry_after = Instant::now() + Duration::from_secs(60);
        cache.password = Some(password?);
    }
    let result = decrypt_and_project(
        envelope,
        cache
            .password
            .as_deref()
            .ok_or(CatalogError::KeyUnavailable)?,
        primary,
        SafeStorage::MacOs,
    );
    if matches!(result, Err(CatalogError::InvalidCatalog)) {
        // A rotated desktop key must be looked up again, without a prompt loop.
        cache.password = None;
        cache.retry_after = Instant::now() + Duration::from_secs(60);
    }
    result
}

fn read_password(
    binary: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Zeroizing<Vec<u8>>, CatalogError> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| CatalogError::KeyUnavailable)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CatalogError::KeyUnavailable);
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Zeroizing::new(Vec::new());
        let result = stdout.take(16385).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + timeout;
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
    Ok(password)
}

/// Decrypt with the platform's safeStorage format and discard desktop credentials.
fn decrypt_and_project(
    envelope: &[u8],
    password: &[u8],
    primary: EnvironmentMetadata,
    storage: SafeStorage,
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
    let (prefix, iterations) = storage.parameters();
    if !ciphertext.starts_with(prefix) {
        return Err(CatalogError::UnsupportedEncryption);
    }
    let mut key = Zeroizing::new([0u8; 16]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", iterations, &mut *key);
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
    #[cfg(unix)]
    #[test]
    fn key_lookup_strips_only_output_newline_and_rejects_failed_or_hung_commands() {
        let key = read_password(
            "/bin/sh",
            &["-c", "printf 'synthetic key\\n'"],
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(key.as_slice(), b"synthetic key");
        for script in ["exit 1", "exit 0"] {
            assert_eq!(
                read_password("/bin/sh", &["-c", script], Duration::from_secs(5)),
                Err(CatalogError::KeyUnavailable)
            );
        }
        assert_eq!(
            read_password(
                "/bin/sh",
                &["-c", "exec sleep 10"],
                Duration::from_millis(100)
            ),
            Err(CatalogError::KeyUnavailable)
        );
    }

    #[test]
    fn malformed_and_unknown_safe_storage_formats_are_rejected() {
        for (ciphertext, expected) in [
            (
                b"v12invalid".as_slice(),
                CatalogError::UnsupportedEncryption,
            ),
            (b"v10".as_slice(), CatalogError::InvalidCatalog),
            (b"v10truncated".as_slice(), CatalogError::InvalidCatalog),
        ] {
            let encrypted = serde_json::to_vec(
                &serde_json::json!({"version":1,"encryptedCatalog":STANDARD.encode(ciphertext)}),
            )
            .unwrap();
            assert_eq!(
                decrypt_and_project(&encrypted, b"synthetic", primary(), SafeStorage::MacOs),
                Err(expected)
            );
        }
    }

    #[test]
    fn macos_safe_storage_catalog_projects_remote_environments_without_credentials() {
        let document = br#"{"schemaVersion":1,"targets":[{"_tag":"RelayConnectionTarget","environmentId":"remote","label":"Other machine"}],"profiles":[],"credentials":[{"credential":{"token":"synthetic-desktop-secret"}}]}"#;
        let mut key = [0u8; 16];
        pbkdf2::pbkdf2_hmac::<sha1::Sha1>(b"synthetic-mac-key", b"saltysalt", 1003, &mut key);
        let mut bytes = b"v10".to_vec();
        bytes.extend(
            cbc::Encryptor::<Aes128>::new(&key.into(), &[b' '; 16].into())
                .encrypt_padded_vec_mut::<Pkcs7>(document),
        );
        let encrypted = serde_json::to_vec(
            &serde_json::json!({"version":1,"encryptedCatalog":STANDARD.encode(bytes)}),
        )
        .unwrap();
        let result = decrypt_and_project(
            &encrypted,
            b"synthetic-mac-key",
            primary(),
            SafeStorage::MacOs,
        )
        .unwrap();
        assert_eq!(
            decrypt_and_project(
                &encrypted,
                b"synthetic-mac-key",
                primary(),
                SafeStorage::Linux
            ),
            Err(CatalogError::UnsupportedEncryption),
        );
        let linux = envelope(r#"{"schemaVersion":1,"targets":[],"profiles":[]}"#);
        assert_eq!(
            decrypt_and_project(&linux, b"synthetic", primary(), SafeStorage::MacOs),
            Err(CatalogError::UnsupportedEncryption),
        );
        assert_eq!(result.len(), 2);
        assert!(matches!(result[1].routes.as_slice(), [Route::Relay]));
        assert!(!format!("{result:?}").contains("synthetic-desktop-secret"));
        assert_eq!(
            decrypt_and_project(&encrypted, b"wrong-key", primary(), SafeStorage::MacOs),
            Err(CatalogError::InvalidCatalog)
        );
    }

    #[test]
    fn decrypts_and_projects_routes_without_credentials() {
        let encrypted = envelope(
            r#"{"schemaVersion":1,"targets":[{"_tag":"RelayConnectionTarget","environmentId":"remote","label":"Remote"},{"_tag":"BearerConnectionTarget","environmentId":"remote","label":"Remote","connectionId":"direct"},{"_tag":"SshConnectionTarget","environmentId":"remote","label":"Remote","connectionId":"ssh"}],"profiles":[{"_tag":"BearerConnectionProfile","connectionId":"direct","environmentId":"remote","httpBaseUrl":"https://example.test/","wsBaseUrl":"wss://example.test/"},{"_tag":"SshConnectionProfile","connectionId":"ssh","environmentId":"remote","target":{"alias":"box","hostname":"host","username":null,"port":22}}],"credentials":[{"credential":{"token":"synthetic-secret"}}],"remoteDpopTokens":[],"disabledEnvironmentIds":["remote"]}"#,
        );
        let result =
            decrypt_and_project(&encrypted, b"synthetic", primary(), SafeStorage::Linux).unwrap();
        assert_eq!(result.len(), 2);
        let mut offline_primary = primary();
        offline_primary.routes.clear();
        let offline = decrypt_and_project(
            &encrypted,
            b"synthetic",
            offline_primary,
            SafeStorage::Linux,
        )
        .unwrap();
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
            decrypt_and_project(&encrypted, b"wrong", primary(), SafeStorage::Linux),
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
            let result = decrypt_and_project(
                &envelope(&document.to_string()),
                b"synthetic",
                primary(),
                SafeStorage::Linux,
            )
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
            decrypt_and_project(&document, b"synthetic", unsafe_primary, SafeStorage::Linux),
            Err(CatalogError::InvalidCatalog)
        );
    }
    #[test]
    fn empty_catalog_is_success_and_bad_version_fails() {
        let encrypted = envelope(
            r#"{"schemaVersion":1,"targets":[],"profiles":[],"credentials":[],"remoteDpopTokens":[]}"#,
        );
        assert_eq!(
            decrypt_and_project(&encrypted, b"synthetic", primary(), SafeStorage::Linux)
                .unwrap()
                .len(),
            1
        );
        let bad = envelope(r#"{"schemaVersion":2,"targets":[],"profiles":[]}"#);
        assert_eq!(
            decrypt_and_project(&bad, b"synthetic", primary(), SafeStorage::Linux),
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
        let result = decrypt_and_project(
            &encrypted,
            b"synthetic",
            local_without_runtime,
            SafeStorage::Linux,
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].routes.is_empty());
        assert!(matches!(result[1].routes.as_slice(), [Route::Relay]));
    }
}
