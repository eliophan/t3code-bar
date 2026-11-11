use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRecord {
    pub environment_id: String,
    pub label: String,
    pub machine: String,
    pub endpoint: String,
    pub enabled: bool,
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

pub fn load(path: &Path) -> Result<Vec<EnvironmentRecord>, RegistryError> {
    let Ok(bytes) = fs::read(path) else { return Ok(Vec::new()) };
    let records: Vec<EnvironmentRecord> = serde_json::from_slice(&bytes)
        .map_err(|_| RegistryError::Invalid("environment registry is invalid".into()))?;
    let mut ids = std::collections::HashSet::new();
    for record in &records {
        validate(record)?;
        if !ids.insert(record.environment_id.clone()) {
            return Err(RegistryError::Invalid("environmentId must be unique".into()));
        }
    }
    Ok(records)
}

pub fn save(path: &Path, records: &[EnvironmentRecord]) -> Result<(), RegistryError> {
    let mut ids = std::collections::HashSet::new();
    for record in records {
        validate(record)?;
        if !ids.insert(record.environment_id.clone()) {
            return Err(RegistryError::Invalid("environmentId must be unique".into()));
        }
    }
    let bytes = serde_json::to_vec_pretty(records).map_err(|_| RegistryError::Io)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|_| RegistryError::Io)?;
    fs::rename(temp, path).map_err(|_| RegistryError::Io)
}
