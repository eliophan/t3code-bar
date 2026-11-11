use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, net::{TcpStream, ToSocketAddrs}, path::Path, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRecord {
    pub environment_id: String,
    pub label: String,
    pub machine: String,
    pub endpoint: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentStatus {
    Disconnected,
    Connecting,
    Reachable,
    Offline,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    InvalidEndpoint,
    Unreachable,
    Unsupported,
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidEndpoint => "invalid environment endpoint",
            Self::Unreachable => "environment is unreachable",
            Self::Unsupported => "environment transport is unsupported",
        })
    }
}

pub trait EnvironmentTransport: Send + Sync {
    fn connect(&self, record: &EnvironmentRecord) -> Result<(), TransportError>;
    fn disconnect(&self, environment_id: &str);
}

#[derive(Default)]
pub struct LocalTransport;

impl EnvironmentTransport for LocalTransport {
    fn connect(&self, record: &EnvironmentRecord) -> Result<(), TransportError> {
        let endpoint = record.endpoint.trim();
        let authority = endpoint
            .strip_prefix("http://")
            .or_else(|| endpoint.strip_prefix("https://"))
            .and_then(|rest| rest.split('/').next())
            .ok_or(TransportError::InvalidEndpoint)?;
        let mut addresses = authority
            .to_socket_addrs()
            .map_err(|_| TransportError::InvalidEndpoint)?;
        let address = addresses.next().ok_or(TransportError::InvalidEndpoint)?;
        TcpStream::connect_timeout(&address, Duration::from_millis(500))
            .map(|_| ())
            .map_err(|_| TransportError::Unreachable)
    }

    fn disconnect(&self, _environment_id: &str) {}
}

pub struct UnsupportedRemoteTransport;

impl EnvironmentTransport for UnsupportedRemoteTransport {
    fn connect(&self, _record: &EnvironmentRecord) -> Result<(), TransportError> {
        Err(TransportError::Unsupported)
    }

    fn disconnect(&self, _environment_id: &str) {}
}

#[derive(Default)]
pub struct EnvironmentRegistry {
    records: Vec<EnvironmentRecord>,
    statuses: HashMap<String, EnvironmentStatus>,
    adapters: HashMap<String, Box<dyn EnvironmentTransport>>,
}

impl EnvironmentRegistry {
    pub fn new(records: Vec<EnvironmentRecord>) -> Result<Self, RegistryError> {
        validate_records(&records)?;
        let statuses = records.iter().map(|r| (r.environment_id.clone(), EnvironmentStatus::Disconnected)).collect();
        Ok(Self { records, statuses, adapters: HashMap::new() })
    }

    pub fn records(&self) -> &[EnvironmentRecord] { &self.records }

    pub fn status(&self, environment_id: &str) -> Option<EnvironmentStatus> {
        self.statuses.get(environment_id).copied()
    }

    pub fn attach<T: EnvironmentTransport + 'static>(&mut self, environment_id: &str, transport: T) -> Result<(), RegistryError> {
        let record = self.records.iter().find(|r| r.environment_id == environment_id).ok_or_else(|| RegistryError::Invalid("environmentId is unknown".into()))?;
        if !record.enabled { return Err(RegistryError::Invalid("environment is disabled".into())); }
        self.adapters.insert(environment_id.to_owned(), Box::new(transport));
        Ok(())
    }

    pub fn connect(&mut self, environment_id: &str) -> Result<(), TransportError> {
        let record = self.records.iter().find(|r| r.environment_id == environment_id && r.enabled).ok_or(TransportError::InvalidEndpoint)?.clone();
        let transport = self.adapters.get(environment_id).ok_or(TransportError::Unsupported)?;
        self.statuses.insert(environment_id.to_owned(), EnvironmentStatus::Connecting);
        match transport.connect(&record) {
            Ok(()) => { self.statuses.insert(environment_id.to_owned(), EnvironmentStatus::Reachable); Ok(()) }
            Err(error) => {
                let status = if error == TransportError::Unsupported { EnvironmentStatus::Unsupported } else { EnvironmentStatus::Offline };
                self.statuses.insert(environment_id.to_owned(), status);
                Err(error)
            }
        }
    }

    pub fn disconnect(&mut self, environment_id: &str) {
        if let Some(transport) = self.adapters.get(environment_id) { transport.disconnect(environment_id); }
        if self.statuses.contains_key(environment_id) { self.statuses.insert(environment_id.to_owned(), EnvironmentStatus::Disconnected); }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    Invalid(String),
    Io,
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Io => f.write_str("environment registry unavailable"),
        }
    }
}

impl std::error::Error for RegistryError {}

fn validate(record: &EnvironmentRecord) -> Result<(), RegistryError> {
    if record.environment_id.trim().is_empty() {
        return Err(RegistryError::Invalid("environmentId is required".into()));
    }
    if record.label.trim().is_empty() {
        return Err(RegistryError::Invalid("label is required".into()));
    }
    if record.endpoint.trim().is_empty() {
        return Err(RegistryError::Invalid("endpoint is required".into()));
    }
    if !record.endpoint.starts_with("https://") && !record.endpoint.starts_with("http://") {
        return Err(RegistryError::Invalid("endpoint must use http or https".into()));
    }
    Ok(())
}

fn validate_records(records: &[EnvironmentRecord]) -> Result<(), RegistryError> {
    let mut ids = std::collections::HashSet::new();
    for record in records {
        validate(record)?;
        if !ids.insert(record.environment_id.clone()) {
            return Err(RegistryError::Invalid("environmentId must be unique".into()));
        }
    }
    Ok(())
}

pub fn load(path: &Path) -> Result<Vec<EnvironmentRecord>, RegistryError> {
    let Ok(bytes) = fs::read(path) else { return Ok(Vec::new()) };
    let records: Vec<EnvironmentRecord> = serde_json::from_slice(&bytes)
        .map_err(|_| RegistryError::Invalid("environment registry is invalid".into()))?;
    validate_records(&records)?;
    Ok(records)
}

pub fn save(path: &Path, records: &[EnvironmentRecord]) -> Result<(), RegistryError> {
    validate_records(records)?;
    let bytes = serde_json::to_vec_pretty(records).map_err(|_| RegistryError::Io)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|_| RegistryError::Io)?;
    fs::rename(temp, path).map_err(|_| RegistryError::Io)
}
