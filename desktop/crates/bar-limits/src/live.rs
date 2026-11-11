use super::{
    merge, parse_record, LimitsContribution, LimitsReader, LimitsSnapshot, LimitsState, Record,
};
use async_trait::async_trait;
use bar_identity::EnvironmentId;
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    fmt, io,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
    time::timeout,
};

const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_HTTP: usize = 1024 * 1024;
const SWIFT_EPOCH: f64 = 978_307_200.0;
const PAIRING_ALPHABET: &str = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

fn is_pairing_code(token: &str) -> bool {
    token.len() == 12
        && token
            .chars()
            .all(|c| PAIRING_ALPHABET.contains(c.to_ascii_uppercase()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionError {
    Destination,
    InvalidToken,
    Authorization,
    PairingRejected,
    Expired,
    Protocol,
    Storage,
    Transport,
    NoSavedCredential,
}
impl fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Destination => "Enter a local T3 Code port from 1 to 65535.",
            Self::InvalidToken => "Enter a T3 Code access token of up to 16 KiB.",
            Self::Authorization => "The access token was rejected by T3 Code.",
            Self::PairingRejected => "T3 Code rejected this pairing key. It may have expired or already been used. Create a new pairing key and try again.",
            Self::Expired => {
                "The access token expired or was revoked. Generate or enter a new token."
            }
            Self::Protocol => "This T3 Code version returned an unsupported Limits stream.",
            Self::Storage => "Could not access the saved pairing key.",
            Self::Transport => "Could not connect to T3 Code.",
            Self::NoSavedCredential => "No saved pairing key. Enter one to connect.",
        })
    }
}
impl std::error::Error for ConnectionError {}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub port: u16,
    token: String,
    expires_at: f64,
}
impl Credential {
    pub fn new(port: u16, token: &str) -> Result<Self, ConnectionError> {
        let token = token.trim();
        if port == 0 {
            return Err(ConnectionError::Destination);
        }
        if token.is_empty() || token.len() > 16384 || token.bytes().any(|b| b < 32 || b == 127) {
            return Err(ConnectionError::InvalidToken);
        }
        let expiry = token
            .split('.')
            .next()
            .and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("exp").and_then(Value::as_f64))
            .filter(|n| n.is_finite() && *n > 0.0)
            .map(|ms| ms / 1000.0 - SWIFT_EPOCH)
            .unwrap_or(63_082_281_600.0);
        Ok(Self {
            port,
            token: token.to_owned(),
            expires_at: expiry,
        })
    }
    fn expired(&self) -> bool {
        !self.expires_at.is_finite()
            || self.expires_at + SWIFT_EPOCH <= Utc::now().timestamp_millis() as f64 / 1000.0
    }
    fn validate(&self) -> Result<(), ConnectionError> {
        if Self::new(self.port, &self.token)?.token != self.token {
            return Err(ConnectionError::InvalidToken);
        }
        if self.expired() {
            return Err(ConnectionError::Expired);
        }
        Ok(())
    }
}
impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("port", &self.port)
            .field("token", &"[redacted]")
            .finish()
    }
}
pub trait CredentialStore: Send + Sync {
    fn load(&self) -> Result<Option<Credential>, ConnectionError>;
    fn save(&self, credential: &Credential) -> Result<(), ConnectionError>;
    fn delete(&self) -> Result<(), ConnectionError>;
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub struct KeyringStore {
    service: String,
    account: String,
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
impl KeyringStore {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }
    pub fn production() -> Self {
        Self::new("com.t3code.bar.tauri.limits", "pairing-key")
    }
    fn entry(&self) -> Result<keyring::Entry, ConnectionError> {
        keyring::Entry::new(&self.service, &self.account).map_err(|_| ConnectionError::Storage)
    }
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
impl CredentialStore for KeyringStore {
    fn load(&self) -> Result<Option<Credential>, ConnectionError> {
        match self.entry()?.get_secret() {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| ConnectionError::Storage),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(ConnectionError::Storage),
        }
    }
    fn save(&self, c: &Credential) -> Result<(), ConnectionError> {
        self.entry()?
            .set_secret(&serde_json::to_vec(c).map_err(|_| ConnectionError::Storage)?)
            .map_err(|_| ConnectionError::Storage)
    }
    fn delete(&self) -> Result<(), ConnectionError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(ConnectionError::Storage),
        }
    }
}

#[async_trait]
pub trait LiveSocket: Send {
    async fn send(&mut self, text: &str) -> Result<(), ConnectionError>;
    async fn receive(&mut self) -> Result<String, ConnectionError>;
}
#[async_trait]
pub trait LiveTransport: Send + Sync {
    /// Converts a T3 pairing code into a bearer session token. Existing bearer
    /// tokens pass through unchanged so saved access tokens remain compatible.
    async fn exchange_pairing(&self, _port: u16, token: &str) -> Result<String, ConnectionError> {
        if is_pairing_code(token.trim()) {
            return Err(ConnectionError::Protocol);
        }
        Ok(token.trim().to_owned())
    }
    async fn ticket(
        &self,
        port: u16,
        token: &str,
    ) -> Result<(String, DateTime<Utc>), ConnectionError>;
    async fn socket(&self, port: u16, ticket: &str)
        -> Result<Box<dyn LiveSocket>, ConnectionError>;
}
#[derive(Default)]
pub struct LoopbackTransport;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind", content = "message")]
pub enum LiveStatus {
    CacheOnly,
    Restoring,
    StorageUnavailable,
    Connecting,
    Live,
    Paused,
    Retrying,
    Failed(String),
}

fn status_for_error(error: ConnectionError) -> LiveStatus {
    if error == ConnectionError::Storage {
        LiveStatus::StorageUnavailable
    } else {
        LiveStatus::Failed(error.to_string())
    }
}
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveSnapshot {
    pub status: LiveStatus,
    pub has_credential: bool,
    pub data: Option<LimitsSnapshot>,
    pub updated_at: Option<DateTime<Utc>>,
}
struct State {
    snapshot: LiveSnapshot,
    credential: Option<Credential>,
    generation: u64,
    task: Option<JoinHandle<()>>,
    live: Option<LiveData>,
}
#[derive(Default, Clone)]
struct LiveData {
    providers: Vec<Record>,
    sources: Vec<Record>,
    notices: Vec<String>,
}
/// Decoder for the existing server configuration stream, without transport or credentials.
#[derive(Default, Clone)]
pub struct LiveLimitsParser(LiveData);

impl LiveLimitsParser {
    pub fn apply(&mut self, event: &Value) -> Result<bool, ConnectionError> {
        self.0.apply(event)
    }

    pub fn contribution(&self, environment_id: EnvironmentId) -> LimitsContribution {
        let mut records = self.0.providers.clone();
        records.extend(self.0.sources.clone());
        LimitsContribution {
            environment_id,
            records,
            notices: self.0.notices.clone(),
            skipped_files: 0,
        }
    }
}
impl LiveData {
    fn apply(&mut self, v: &Value) -> Result<bool, ConnectionError> {
        if v.get("version").and_then(Value::as_f64) != Some(1.0) {
            return Err(ConnectionError::Protocol);
        }
        let kind = v
            .get("type")
            .and_then(Value::as_str)
            .ok_or(ConnectionError::Protocol)?;
        match kind {
            "snapshot" => {
                let c = v.get("config").ok_or(ConnectionError::Protocol)?;
                self.providers = providers(c.get("providers"))?;
                if let Some(s) = c.get("usageLimitSources") {
                    self.decode_sources(s)?;
                } else {
                    self.sources.clear();
                    self.notices.clear();
                }
            }
            "providerStatuses" => self.providers = providers(v.pointer("/payload/providers"))?,
            "usageLimitSourcesUpdated" => self.decode_sources(
                v.pointer("/payload/sources")
                    .ok_or(ConnectionError::Protocol)?,
            )?,
            "keybindingsUpdated" | "settingsUpdated" | "environmentThemesUpdated" => {
                return Ok(false)
            }
            _ => return Err(ConnectionError::Protocol),
        }
        Ok(true)
    }
    fn decode_sources(&mut self, value: &Value) -> Result<(), ConnectionError> {
        let sources = value.as_array().ok_or(ConnectionError::Protocol)?;
        let mut records = Vec::new();
        let mut notices = Vec::new();
        for source in sources {
            let (Some(id), Some(label), Some(accounts)) = (
                source.get("id").and_then(Value::as_str),
                source.get("label").and_then(Value::as_str),
                source.get("accounts").and_then(Value::as_array),
            ) else {
                continue;
            };
            if let Some(e) = source.get("error").and_then(Value::as_str) {
                notices.push(format!("{label}: {e}"));
            } else if accounts.is_empty() {
                notices.push(format!("{label}: No accounts reported."));
            }
            for a in accounts {
                let (Some(account), Some(driver), Some(limits)) = (
                    a.get("id").and_then(Value::as_str),
                    a.get("driver").and_then(Value::as_str),
                    a.get("usageLimits").filter(|v| v.is_object()),
                ) else {
                    continue;
                };
                let workspace = a.get("workspaceId").and_then(Value::as_str);
                let email = a.get("email").and_then(Value::as_str);
                let name = if account.to_lowercase().ends_with(".json") {
                    &account[..account.len() - 5]
                } else {
                    account
                };
                let raw = json!({"instanceId":format!("source:{}:{id}:{}:{account}:{}", id.len(), account.len(), workspace.map(|w| format!("{}:{w}", w.len())).unwrap_or_else(|| "none".to_owned())), "driver":driver,"enabled":true,"installed":true,"displayName":if email.is_none(){Some(name)}else{None},"auth":{"status":"authenticated","label":a.get("plan"),"email":email,"workspaceId":workspace},"usageLimits":limits});
                if let Some(mut record) = parse_record(&raw, "source") {
                    record.is_source = true;
                    records.push(record);
                }
            }
        }
        self.sources = records;
        self.notices = notices;
        Ok(())
    }
    fn snapshot(&self, cache: &[Record], skipped: usize) -> LimitsSnapshot {
        let mut records = self.providers.clone();
        records.extend_from_slice(cache);
        records.extend(self.sources.clone());
        let mut snapshot = merge(&records, skipped);
        snapshot.notices.splice(0..0, self.notices.clone());
        if !snapshot.notices.is_empty() {
            snapshot.state = LimitsState::Ok;
        }
        snapshot
    }
}
fn providers(v: Option<&Value>) -> Result<Vec<Record>, ConnectionError> {
    let values = v
        .and_then(Value::as_array)
        .ok_or(ConnectionError::Protocol)?;
    if !values.iter().all(Value::is_object) {
        return Err(ConnectionError::Protocol);
    }
    Ok(values
        .iter()
        .filter_map(|v| parse_record(v, "provider"))
        .collect())
}
impl LimitsReader {
    /// Return the raw live and cache reports for one environment, before account merging.
    pub fn live_contribution(
        &self,
        connection: &LimitsConnection,
        environment_id: EnvironmentId,
    ) -> Option<LimitsContribution> {
        let state = connection.inner.lock().unwrap();
        let live = state.live.as_ref()?;
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        let cache: Vec<_> = entries.iter().filter_map(|(_, v)| v.2.clone()).collect();
        let mut records = live.providers.clone();
        if state.snapshot.status != LiveStatus::Live {
            records.extend(cache.iter().cloned());
        }
        records.extend(live.sources.clone());
        Some(LimitsContribution {
            environment_id,
            records,
            notices: live.notices.clone(),
            skipped_files: entries.len() - cache.len(),
        })
    }
    pub fn merge_live(&self, connection: &LimitsConnection) -> Option<LimitsSnapshot> {
        let state = connection.inner.lock().unwrap();
        let live = state.live.as_ref()?;
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_by_key(|(k, _)| *k);
        let cache: Vec<_> = entries.iter().filter_map(|(_, v)| v.2.clone()).collect();
        Some(live.snapshot(
            if state.snapshot.status == LiveStatus::Live {
                &[]
            } else {
                &cache
            },
            entries.len() - cache.len(),
        ))
    }
}
#[derive(Clone)]
pub struct LimitsConnection {
    inner: Arc<Mutex<State>>,
    store: Arc<dyn CredentialStore>,
    transport: Arc<dyn LiveTransport>,
    retry: Duration,
}
impl LimitsConnection {
    pub fn new(store: Arc<dyn CredentialStore>, transport: Arc<dyn LiveTransport>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                snapshot: LiveSnapshot {
                    status: LiveStatus::CacheOnly,
                    has_credential: false,
                    data: None,
                    updated_at: None,
                },
                credential: None,
                generation: 0,
                task: None,
                live: None,
            })),
            store,
            transport,
            retry: Duration::from_secs(3),
        }
    }
    pub fn with_default_transport(store: Arc<dyn CredentialStore>) -> Self {
        Self::new(store, Arc::new(LoopbackTransport))
    }
    pub fn with_retry_delay(mut self, retry: Duration) -> Self {
        self.retry = retry;
        self
    }
    pub fn snapshot(&self) -> LiveSnapshot {
        let state = self.inner.lock().unwrap();
        let mut snapshot = state.snapshot.clone();
        snapshot.has_credential =
            state.credential.is_some() && !matches!(snapshot.status, LiveStatus::Failed(_));
        snapshot
    }
    fn stop_locked(state: &mut State) {
        state.generation = state.generation.wrapping_add(1);
        if let Some(task) = state.task.take() {
            task.abort();
        }
    }
    fn fail(&self, e: ConnectionError) -> ConnectionError {
        self.inner.lock().unwrap().snapshot.status = status_for_error(e);
        e
    }
    fn load_credential(&self) -> Result<Option<Credential>, ConnectionError> {
        // Publish before the OS call: a Keychain approval may wait indefinitely.
        self.inner.lock().unwrap().snapshot.status = LiveStatus::Restoring;
        let credential = self.store.load().map_err(|e| self.fail(e))?;
        if let Some(c) = &credential {
            if let Err(e) = c.validate() {
                if matches!(e, ConnectionError::Expired | ConnectionError::InvalidToken)
                    && self.store.delete().is_err()
                {
                    return Err(self.fail(ConnectionError::Storage));
                }
                return Err(self.fail(e));
            }
        }
        Ok(credential)
    }
    pub fn restore_saved(&self) -> Result<(), ConnectionError> {
        let credential = self.load_credential()?;
        let mut state = self.inner.lock().unwrap();
        Self::stop_locked(&mut state);
        state.snapshot.status = if credential.is_some() {
            LiveStatus::Paused
        } else {
            LiveStatus::CacheOnly
        };
        state.credential = credential;
        state.live = None;
        state.snapshot.data = None;
        state.snapshot.updated_at = None;
        Ok(())
    }
    pub async fn connect(&self, port: u16, token: &str) -> Result<(), ConnectionError> {
        Credential::new(port, token)?.validate()?;
        let access_token = timeout(
            Duration::from_secs(15),
            self.transport.exchange_pairing(port, token),
        )
        .await
        .map_err(|_| ConnectionError::Transport)??;
        let c = Credential::new(port, &access_token)?;
        c.validate()?;
        {
            let mut s = self.inner.lock().unwrap();
            Self::stop_locked(&mut s);
            if let Err(e) = self.store.save(&c) {
                s.snapshot.status = status_for_error(e);
                return Err(e);
            }
            s.credential = Some(c);
            s.live = None;
            s.snapshot.data = None;
            s.snapshot.updated_at = None;
        }
        self.resume().await
    }
    pub async fn reconnect_saved(&self, port: u16) -> Result<(), ConnectionError> {
        self.reconnect(port, false).await
    }
    pub async fn reconnect_saved_if_available(&self, port: u16) -> Result<(), ConnectionError> {
        self.reconnect(port, true).await
    }
    async fn reconnect(&self, port: u16, optional: bool) -> Result<(), ConnectionError> {
        if optional
            && matches!(
                self.snapshot().status,
                LiveStatus::Live | LiveStatus::Connecting
            )
        {
            return Ok(());
        }
        {
            let mut s = self.inner.lock().unwrap();
            Self::stop_locked(&mut s);
        }
        if port == 0 {
            return Err(self.fail(ConnectionError::Destination));
        }
        let c = self.load_credential()?;
        let Some(c) = c else {
            {
                let mut s = self.inner.lock().unwrap();
                s.credential = None;
                s.live = None;
                s.snapshot.data = None;
                s.snapshot.updated_at = None;
                s.snapshot.status = LiveStatus::CacheOnly;
            }
            return if optional {
                Ok(())
            } else {
                Err(self.fail(ConnectionError::NoSavedCredential))
            };
        };
        if c.port != port {
            return Err(self.fail(ConnectionError::Destination));
        }
        {
            let mut s = self.inner.lock().unwrap();
            s.credential = Some(c);
            s.live = None;
            s.snapshot.data = None;
            s.snapshot.updated_at = None;
        }
        self.resume().await
    }
    pub async fn pause(&self) {
        let mut s = self.inner.lock().unwrap();
        Self::stop_locked(&mut s);
        if s.credential.is_some() && !matches!(s.snapshot.status, LiveStatus::Failed(_)) {
            s.snapshot.status = LiveStatus::Paused;
        }
    }
    pub async fn disconnect(&self) -> Result<(), ConnectionError> {
        let mut s = self.inner.lock().unwrap();
        Self::stop_locked(&mut s);
        if let Err(e) = self.store.delete() {
            s.snapshot.status = status_for_error(e);
            return Err(e);
        }
        s.credential = None;
        s.live = None;
        s.snapshot.data = None;
        s.snapshot.updated_at = None;
        s.snapshot.status = LiveStatus::CacheOnly;
        Ok(())
    }
    pub async fn resume(&self) -> Result<(), ConnectionError> {
        let mut s = self.inner.lock().unwrap();
        if s.task.as_ref().is_some_and(|task| task.is_finished()) {
            s.task = None;
        }
        if s.task.is_some() {
            return Ok(());
        }
        let Some(c) = s.credential.clone() else {
            return Ok(());
        };
        s.generation = s.generation.wrapping_add(1);
        let generation = s.generation;
        s.snapshot.status = LiveStatus::Connecting;
        let this = self.clone();
        s.task = Some(tokio::spawn(async move {
            this.run(c, generation).await;
        }));
        Ok(())
    }
    fn update(&self, generation: u64, f: impl FnOnce(&mut State)) -> bool {
        let mut s = self.inner.lock().unwrap();
        if s.generation != generation {
            return false;
        }
        f(&mut s);
        true
    }
    async fn run(&self, c: Credential, generation: u64) {
        loop {
            let result = self.stream(&c, generation).await;
            if matches!(result, Err(ConnectionError::Transport)) {
                if !self.update(generation, |s| s.snapshot.status = LiveStatus::Retrying) {
                    return;
                }
                tokio::time::sleep(self.retry).await;
                continue;
            }
            if let Err(e) = result {
                self.update(generation, |s| {
                    if matches!(
                        e,
                        ConnectionError::Expired
                            | ConnectionError::Authorization
                            | ConnectionError::InvalidToken
                    ) {
                        s.credential = None;
                    }
                    s.snapshot.status = status_for_error(e);
                    s.task = None;
                });
                if matches!(
                    e,
                    ConnectionError::Expired
                        | ConnectionError::Authorization
                        | ConnectionError::InvalidToken
                ) {
                    let mut s = self.inner.lock().unwrap();
                    if s.generation == generation && self.store.delete().is_err() {
                        s.snapshot.status = status_for_error(ConnectionError::Storage);
                    }
                }
            }
            return;
        }
    }
    async fn stream(&self, c: &Credential, generation: u64) -> Result<(), ConnectionError> {
        c.validate()?;
        let (ticket, expiry) = timeout(
            Duration::from_secs(15),
            self.transport.ticket(c.port, &c.token),
        )
        .await
        .map_err(|_| ConnectionError::Transport)??;
        if ticket.is_empty() || expiry <= Utc::now() {
            return Err(ConnectionError::Protocol);
        }
        let mut socket = timeout(
            Duration::from_secs(15),
            self.transport.socket(c.port, &ticket),
        )
        .await
        .map_err(|_| ConnectionError::Transport)??;
        socket.send(&json!({"_tag":"Request","id":0,"tag":"subscribeServerConfig","payload":{"usageLimitSources":true,"workspaceSnapshotsOnDemand":true},"headers":[]}).to_string()).await?;
        let mut data = LiveData::default();
        let start = Instant::now();
        let mut last = start;
        let mut has_snapshot = false;
        loop {
            c.validate()?;
            if !has_snapshot && start.elapsed() > Duration::from_secs(15) {
                return Err(ConnectionError::Transport);
            }
            let received = timeout(Duration::from_secs(5), socket.receive()).await;
            let text = match received {
                Ok(result) => result?,
                Err(_) => {
                    c.validate()?;
                    if last.elapsed() > Duration::from_secs(15)
                        || (!has_snapshot && start.elapsed() > Duration::from_secs(15))
                    {
                        return Err(ConnectionError::Transport);
                    }
                    socket.send(r#"{"_tag":"Ping"}"#).await?;
                    continue;
                }
            };
            last = Instant::now();
            if text.len() > MAX_FRAME {
                return Err(ConnectionError::Protocol);
            }
            let raw: Value = serde_json::from_str(&text).map_err(|_| ConnectionError::Protocol)?;
            let frames = match raw {
                Value::Array(v) => v,
                Value::Object(_) => vec![raw],
                _ => return Err(ConnectionError::Protocol),
            };
            for frame in frames {
                match frame.get("_tag").and_then(Value::as_str) {
                    Some("Pong") => continue,
                    Some("Chunk")
                        if frame.get("requestId").and_then(Value::as_f64) == Some(0.0) => {}
                    _ => return Err(ConnectionError::Protocol),
                }
                let values = frame
                    .get("values")
                    .and_then(Value::as_array)
                    .ok_or(ConnectionError::Protocol)?;
                let mut changed = false;
                for v in values {
                    if v.get("type").and_then(Value::as_str) == Some("snapshot") {
                        has_snapshot = true;
                    }
                    if !has_snapshot {
                        return Err(ConnectionError::Protocol);
                    }
                    changed |= data.apply(v)?;
                }
                socket.send(r#"{"_tag":"Ack","requestId":0}"#).await?;
                if changed
                    && !self.update(generation, |s| {
                        s.snapshot.data = Some(data.snapshot(&[], 0));
                        s.snapshot.updated_at = Some(Utc::now());
                        s.snapshot.status = LiveStatus::Live;
                        s.live = Some(data.clone());
                    })
                {
                    return Ok(());
                }
            }
        }
    }
}

fn transport_error(_: io::Error) -> ConnectionError {
    ConnectionError::Transport
}
async fn open(port: u16) -> Result<TcpStream, ConnectionError> {
    if port == 0 {
        return Err(ConnectionError::Destination);
    }
    timeout(
        Duration::from_secs(15),
        TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)),
    )
    .await
    .map_err(|_| ConnectionError::Transport)?
    .map_err(transport_error)
}
async fn headers(stream: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), ConnectionError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buf.windows(4).position(|v| v == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok((buf, rest));
        }
        if buf.len() >= 32768 {
            return Err(ConnectionError::Protocol);
        }
        let n = stream.read(&mut chunk).await.map_err(transport_error)?;
        if n == 0 {
            return Err(ConnectionError::Transport);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}
fn parse_headers(bytes: &[u8]) -> Result<(u16, Vec<(String, String)>), ConnectionError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ConnectionError::Protocol)?;
    let mut lines = text.split("\r\n");
    let mut first = lines
        .next()
        .ok_or(ConnectionError::Protocol)?
        .split_whitespace();
    if !matches!(first.next(), Some("HTTP/1.1" | "HTTP/1.0")) {
        return Err(ConnectionError::Protocol);
    }
    let status = first
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or(ConnectionError::Protocol)?;
    let mut result = Vec::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let (k, v) = line.split_once(':').ok_or(ConnectionError::Protocol)?;
        result.push((k.to_ascii_lowercase(), v.trim().to_owned()));
    }
    Ok((status, result))
}
fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

async fn read_body(
    stream: &mut TcpStream,
    headers: &[(String, String)],
    mut body: Vec<u8>,
) -> Result<Vec<u8>, ConnectionError> {
    if let Some(length) = header(headers, "content-length") {
        let n: usize = length.parse().map_err(|_| ConnectionError::Protocol)?;
        if n >= MAX_HTTP {
            return Err(ConnectionError::Protocol);
        }
        while body.len() < n {
            let mut chunk = [0u8; 8192];
            let count = stream.read(&mut chunk).await.map_err(transport_error)?;
            if count == 0 {
                return Err(ConnectionError::Transport);
            }
            body.extend_from_slice(&chunk[..count]);
            if body.len() > MAX_HTTP {
                return Err(ConnectionError::Protocol);
            }
        }
        body.truncate(n);
    } else {
        let mut limited = (&mut *stream).take(MAX_HTTP as u64 + 1);
        limited
            .read_to_end(&mut body)
            .await
            .map_err(transport_error)?;
        if body.len() >= MAX_HTTP {
            return Err(ConnectionError::Protocol);
        }
        if header(headers, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
            body = decode_chunks(&body)?;
        }
    }
    Ok(body)
}
#[async_trait]
impl LiveTransport for LoopbackTransport {
    async fn exchange_pairing(&self, port: u16, token: &str) -> Result<String, ConnectionError> {
        let token = token.trim();
        if !is_pairing_code(token) {
            return Ok(token.to_owned());
        }
        let mut stream = open(port).await?;
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair(
                "grant_type",
                "urn:ietf:params:oauth:grant-type:token-exchange",
            )
            .append_pair("subject_token", &token.to_ascii_uppercase())
            .append_pair(
                "subject_token_type",
                "urn:t3:params:oauth:token-type:environment-bootstrap",
            )
            .append_pair(
                "requested_token_type",
                "urn:ietf:params:oauth:token-type:access_token",
            )
            .append_pair("client_label", "T3 Code Bar")
            .append_pair("scope", "orchestration:read")
            .append_pair("client_device_type", "desktop")
            .append_pair("client_os", std::env::consts::OS)
            .finish();
        let request = format!(
            "POST /oauth/token HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{form}",
            form.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(transport_error)?;
        let (head, body) = headers(&mut stream).await?;
        let (status, h) = parse_headers(&head)?;
        let body = read_body(&mut stream, &h, body).await?;
        match status {
            200 => (),
            400..=403 => return Err(ConnectionError::PairingRejected),
            300..=499 => return Err(ConnectionError::Protocol),
            _ => return Err(ConnectionError::Transport),
        }
        let data: Value = serde_json::from_slice(&body).map_err(|_| ConnectionError::Protocol)?;
        if data.get("token_type").and_then(Value::as_str) != Some("Bearer")
            || !data
                .get("expires_in")
                .and_then(Value::as_f64)
                .is_some_and(|seconds| seconds.is_finite() && seconds > 0.0)
        {
            return Err(ConnectionError::Protocol);
        }
        data.get("access_token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or(ConnectionError::Protocol)
    }
    async fn ticket(
        &self,
        port: u16,
        token: &str,
    ) -> Result<(String, DateTime<Utc>), ConnectionError> {
        let mut stream = open(port).await?;
        let request = format!("POST /api/auth/websocket-ticket/rpc HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(transport_error)?;
        let (head, mut body) = headers(&mut stream).await?;
        let (status, h) = parse_headers(&head)?;
        match status {
            200 => (),
            401 | 403 => return Err(ConnectionError::Expired),
            300..=499 => return Err(ConnectionError::Authorization),
            _ => return Err(ConnectionError::Transport),
        }
        body = read_body(&mut stream, &h, body).await?;
        let data: Value = serde_json::from_slice(&body).map_err(|_| ConnectionError::Protocol)?;
        let ticket = data
            .get("ticket")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(ConnectionError::Protocol)?
            .to_owned();
        let expiry = data
            .get("expiresAt")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .ok_or(ConnectionError::Protocol)?
            .with_timezone(&Utc);
        Ok((ticket, expiry))
    }
    async fn socket(
        &self,
        port: u16,
        ticket: &str,
    ) -> Result<Box<dyn LiveSocket>, ConnectionError> {
        let mut stream = open(port).await?;
        let mut nonce = [0; 16];
        rand::thread_rng().fill_bytes(&mut nonce);
        let key = STANDARD.encode(nonce);
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("wsTicket", ticket)
            .append_pair("orchestrationProtocol", "2")
            .finish();
        let request = format!("GET /ws?{query} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(transport_error)?;
        let (head, buf) = headers(&mut stream).await?;
        let (status, h) = parse_headers(&head)?;
        let accept = STANDARD.encode(Sha1::digest(
            format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
        ));
        if status != 101
            || header(&h, "sec-websocket-accept") != Some(accept.as_str())
            || !header(&h, "upgrade").is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
            || !header(&h, "connection").is_some_and(|v| {
                v.split(',')
                    .any(|s| s.trim().eq_ignore_ascii_case("upgrade"))
            })
        {
            return Err(ConnectionError::Protocol);
        }
        Ok(Box::new(WsSocket {
            stream,
            buf,
            fragmented: Vec::new(),
            fragment_opcode: None,
        }))
    }
}
fn decode_chunks(input: &[u8]) -> Result<Vec<u8>, ConnectionError> {
    let mut pos = 0;
    let mut result = Vec::new();
    loop {
        let end = input[pos..]
            .windows(2)
            .position(|v| v == b"\r\n")
            .ok_or(ConnectionError::Protocol)?
            + pos;
        let size = std::str::from_utf8(&input[pos..end])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or(""), 16).ok())
            .ok_or(ConnectionError::Protocol)?;
        pos = end + 2;
        if size == 0 {
            return Ok(result);
        }
        let next = pos.checked_add(size).ok_or(ConnectionError::Protocol)?;
        let framed_end = next.checked_add(2).ok_or(ConnectionError::Protocol)?;
        if framed_end > input.len()
            || &input[next..framed_end] != b"\r\n"
            || size >= MAX_HTTP.saturating_sub(result.len())
        {
            return Err(ConnectionError::Protocol);
        }
        result.extend_from_slice(&input[pos..next]);
        pos = framed_end;
    }
}
struct WsSocket {
    stream: TcpStream,
    buf: Vec<u8>,
    fragmented: Vec<u8>,
    fragment_opcode: Option<u8>,
}
impl WsSocket {
    async fn write_frame(&mut self, opcode: u8, data: &[u8]) -> Result<(), ConnectionError> {
        if data.len() > MAX_FRAME {
            return Err(ConnectionError::Protocol);
        }
        let mut frame = Vec::with_capacity(data.len() + 14);
        frame.push(0x80 | opcode);
        if data.len() < 126 {
            frame.push(0x80 | data.len() as u8);
        } else if data.len() <= 65535 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(data.len() as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(data.len() as u64).to_be_bytes());
        }
        let mut mask = [0; 4];
        rand::thread_rng().fill_bytes(&mut mask);
        frame.extend_from_slice(&mask);
        frame.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.stream.write_all(&frame).await.map_err(transport_error)
    }
    async fn fill(&mut self, n: usize) -> Result<(), ConnectionError> {
        while self.buf.len() < n {
            let mut chunk = [0; 8192];
            let count = self
                .stream
                .read(&mut chunk)
                .await
                .map_err(transport_error)?;
            if count == 0 {
                return Err(ConnectionError::Transport);
            }
            self.buf.extend_from_slice(&chunk[..count]);
        }
        Ok(())
    }
}
#[async_trait]
impl LiveSocket for WsSocket {
    async fn send(&mut self, text: &str) -> Result<(), ConnectionError> {
        self.write_frame(1, text.as_bytes()).await
    }
    async fn receive(&mut self) -> Result<String, ConnectionError> {
        loop {
            self.fill(2).await?;
            let a = self.buf[0];
            let b = self.buf[1];
            if a & 0x70 != 0 || b & 0x80 != 0 {
                return Err(ConnectionError::Protocol);
            }
            let opcode = a & 0x0f;
            let fin = a & 0x80 != 0;
            let extra = match b & 0x7f {
                126 => 2,
                127 => 8,
                _ => 0,
            };
            self.fill(2 + extra).await?;
            let len = match extra {
                2 => u16::from_be_bytes([self.buf[2], self.buf[3]]) as u64,
                8 => u64::from_be_bytes(self.buf[2..10].try_into().unwrap()),
                _ => (b & 0x7f) as u64,
            };
            if len > MAX_FRAME as u64
                || (opcode >= 8 && (!fin || len > 125))
                || (extra == 2 && len < 126)
                || (extra == 8 && len <= 65535)
            {
                return Err(ConnectionError::Protocol);
            }
            let end = 2 + extra + len as usize;
            self.fill(end).await?;
            let data = self.buf[2 + extra..end].to_vec();
            self.buf.drain(..end);
            match opcode {
                8 => return Err(ConnectionError::Transport),
                9 => {
                    self.write_frame(10, &data).await?;
                    continue;
                }
                10 => continue,
                1 | 2 if self.fragment_opcode.is_none() => {
                    if fin {
                        return String::from_utf8(data).map_err(|_| ConnectionError::Protocol);
                    }
                    self.fragment_opcode = Some(opcode);
                    self.fragmented = data;
                }
                0 if self.fragment_opcode.is_some() => {
                    if self.fragmented.len() + data.len() > MAX_FRAME {
                        return Err(ConnectionError::Protocol);
                    }
                    self.fragmented.extend_from_slice(&data);
                    if fin {
                        self.fragment_opcode = None;
                        return String::from_utf8(std::mem::take(&mut self.fragmented))
                            .map_err(|_| ConnectionError::Protocol);
                    }
                }
                _ => return Err(ConnectionError::Protocol),
            }
        }
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
