//! Read-only transport to an already-running T3 server over OpenSSH.
//!
//! This module deliberately does not launch or stop the remote server, or inspect credentials
//! on the remote host. It reads the state written by the upstream SSH launcher,
//! validates that the named process is alive, and owns only the local tunnel.

use crate::{catalog::SshTarget, ConnectionError, VerifiedEndpoint};
use bar_identity::EnvironmentId;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{BufRead, BufReader, Read},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use url::Url;

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_TUNNEL_START_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_OUTPUT: usize = 64 * 1024;
const MAX_PID: u32 = i32::MAX as u32;

/// Limits and executable used by the SSH route.
#[derive(Clone, Debug)]
pub struct SshConnectOptions {
    /// The OpenSSH executable. Tests use a disposable fake executable here.
    pub ssh_program: PathBuf,
    pub command_timeout: Duration,
    pub tunnel_start_timeout: Duration,
    pub max_output_bytes: usize,
}

impl Default for SshConnectOptions {
    fn default() -> Self {
        Self {
            ssh_program: PathBuf::from("ssh"),
            command_timeout: DEFAULT_COMMAND_TIMEOUT,
            tunnel_start_timeout: DEFAULT_TUNNEL_START_TIMEOUT,
            max_output_bytes: DEFAULT_MAX_OUTPUT,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SshRouteError {
    InvalidTarget,
    MetadataUnavailable,
    MalformedMetadata,
    NoRunningServer,
    CommandTimeout,
    OutputLimit,
    CommandFailed { status: Option<i32>, stderr: String },
    Spawn(String),
    TunnelExited { status: Option<i32>, stderr: String },
    TunnelTimeout,
    Io(String),
    Runtime(String),
}

impl fmt::Display for SshRouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTarget => f.write_str("SSH target is invalid."),
            Self::MetadataUnavailable => f.write_str("SSH metadata is unavailable."),
            Self::MalformedMetadata => f.write_str("SSH server metadata is malformed."),
            Self::NoRunningServer => f.write_str("No running T3 server was found."),
            Self::CommandTimeout => f.write_str("SSH command timed out."),
            Self::OutputLimit => f.write_str("SSH command output exceeded its limit."),
            Self::CommandFailed { status, stderr } => {
                write!(f, "SSH metadata command failed ({status:?}): {stderr}")
            }
            Self::Spawn(message) => write!(f, "failed to spawn SSH: {message}"),
            Self::TunnelExited { status, stderr } => {
                write!(f, "SSH tunnel exited ({status:?}): {stderr}")
            }
            Self::TunnelTimeout => f.write_str("SSH tunnel did not start before the deadline."),
            Self::Io(message) | Self::Runtime(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SshRouteError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RemoteServer {
    pid: u32,
    port: u16,
}

/// A local loopback endpoint backed by one Bar-owned SSH child process.
pub struct SshTunnel {
    local_addr: SocketAddr,
    child: Option<Child>,
    /// Hash of the exact validated target. Unlike `local_addr`, it is the same
    /// for every tunnel to that target.
    identity: String,
}

impl SshTunnel {
    /// Discover the descriptor through this tunnel and bind the endpoint's
    /// credentials to the stable SSH target identity. The descriptor must name
    /// `expected` before any credential is read, sent, or stored.
    pub async fn verified_endpoint(
        &self,
        expected: &EnvironmentId,
    ) -> Result<VerifiedEndpoint, ConnectionError> {
        let mut endpoint = VerifiedEndpoint::discover(&self.http_base_url(), expected).await?;
        endpoint.credential_identity = Some(self.identity.clone());
        Ok(endpoint)
    }

    /// Identify a new SSH destination without reading or sending credentials.
    pub async fn identify_endpoint(&self) -> Result<VerifiedEndpoint, ConnectionError> {
        let mut endpoint = VerifiedEndpoint::identify(&self.http_base_url()).await?;
        endpoint.credential_identity = Some(self.identity.clone());
        Ok(endpoint)
    }

    /// Whether the owned OpenSSH child is still running.
    pub fn is_running(&mut self) -> bool {
        self.child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn http_base_url(&self) -> String {
        format!("http://{}/", self.local_addr)
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        // This is the only process this module owns. Kill and wait make drop
        // idempotent and prevent an orphaned local forward after route switch.
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Connect synchronously without touching the remote T3 server lifecycle.
pub fn connect(
    target: &SshTarget,
    options: &SshConnectOptions,
) -> Result<SshTunnel, SshRouteError> {
    let target = ValidatedTarget::new(target)?;
    let key = state_key(target.raw);
    let metadata = read_metadata(&target, &key, options)?;
    let local_listener = TcpListener::bind((IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .map_err(|error| SshRouteError::Io(error.to_string()))?;
    let local_addr = local_listener
        .local_addr()
        .map_err(|error| SshRouteError::Io(error.to_string()))?;
    drop(local_listener);

    let args = tunnel_args(&target, local_addr.port(), metadata.port);
    let child = spawn_ssh(&options.ssh_program, &args)?;
    let mut tunnel = SshTunnel {
        local_addr,
        child: Some(child),
        identity: target_identity(target.raw),
    };
    let child = tunnel.child.as_mut().expect("owned child");
    let stderr = child.stderr.take();
    let stderr_rx = spawn_tunnel_reader(stderr, options.max_output_bytes);
    let expected = format!(
        "Local forwarding listening on 127.0.0.1 port {}.",
        local_addr.port()
    );
    let mut diagnostic = String::new();
    let mut bound = false;
    let deadline = Instant::now() + options.tunnel_start_timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                while let Ok(line) = stderr_rx.recv_timeout(Duration::from_millis(20)) {
                    diagnostic.push_str(&line?);
                }
                return Err(SshRouteError::TunnelExited {
                    status: status.code(),
                    stderr: diagnostic,
                });
            }
            Ok(None) if Instant::now() >= deadline => return Err(SshRouteError::TunnelTimeout),
            Ok(None) => {
                while let Ok(line) = stderr_rx.try_recv() {
                    let line = line?;
                    bound |= line.contains(&expected);
                    diagnostic.push_str(&line);
                }
                if bound
                    && TcpStream::connect_timeout(&local_addr, Duration::from_millis(10)).is_ok()
                {
                    return Ok(tunnel);
                }
                thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(error) => return Err(SshRouteError::Io(error.to_string())),
        }
    }
}

/// Async adapter for the host's persistent Tokio runtime.
pub async fn connect_async(
    target: SshTarget,
    options: SshConnectOptions,
) -> Result<SshTunnel, SshRouteError> {
    tokio::task::spawn_blocking(move || connect(&target, &options))
        .await
        .map_err(|error| SshRouteError::Runtime(error.to_string()))?
}

/// The upstream state key is SHA-256(alias NUL hostname NUL user-or-empty NUL
/// port-or-empty), represented by its first 16 hexadecimal characters.
pub fn state_key(target: &SshTarget) -> String {
    let input = format!(
        "{}\0{}\0{}\0{}",
        target.alias,
        target.hostname,
        target.username.as_deref().unwrap_or(""),
        target.port.map(|port| port.to_string()).unwrap_or_default()
    );
    let digest = Sha256::digest(input.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Domain-separated SHA-256 over length-prefixed target fields. Absent optional
/// fields differ from empty ones.
fn target_identity(target: &SshTarget) -> String {
    let mut hash = Sha256::new();
    hash.update(b"t3-code-bar ssh target identity v1\0");
    let mut field = |value: Option<&[u8]>| match value {
        Some(bytes) => {
            hash.update([1]);
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        }
        None => hash.update([0]),
    };
    field(Some(target.alias.as_bytes()));
    field(Some(target.hostname.as_bytes()));
    field(target.username.as_deref().map(str::as_bytes));
    field(
        target
            .port
            .map(|port| port.to_be_bytes())
            .as_ref()
            .map(|b| b.as_slice()),
    );
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The stable credential identity of a target, available without starting a
/// tunnel. Invalid targets have none.
pub(crate) fn credential_identity(target: &SshTarget) -> Result<String, SshRouteError> {
    ValidatedTarget::new(target)?;
    Ok(target_identity(target))
}

struct ValidatedTarget<'a> {
    raw: &'a SshTarget,
    host_spec: String,
}

impl<'a> ValidatedTarget<'a> {
    fn new(target: &'a SshTarget) -> Result<Self, SshRouteError> {
        if !valid_atom(&target.alias, false)
            || !valid_atom(&target.hostname, false)
            || target.hostname.is_empty()
            || target.port == Some(0)
            || target
                .username
                .as_deref()
                .is_some_and(|username| username.is_empty() || !valid_atom(username, true))
        {
            return Err(SshRouteError::InvalidTarget);
        }
        let destination = if target.alias.is_empty() {
            target.hostname.as_str()
        } else {
            target.alias.as_str()
        };
        let host_spec = match target.username.as_deref() {
            Some(username) => format!("{username}@{destination}"),
            None => destination.to_owned(),
        };
        Ok(Self {
            raw: target,
            host_spec,
        })
    }
}

fn valid_atom(value: &str, username: bool) -> bool {
    !value.chars().any(|ch| {
        ch.is_whitespace()
            || ch.is_control()
            || ch == '@'
            || ch == '\''
            || ch == '"'
            || ch == '`'
            || ch == ';'
            || ch == '&'
            || ch == '|'
            || ch == '$'
            || ch == '<'
            || ch == '>'
            || (!username && ch == '/')
    })
}

fn metadata_script(key: &str) -> String {
    // key is generated locally and contains only lowercase hexadecimal bytes.
    format!(
        r#"set -u; d="$HOME/.t3/ssh-launch/{key}"; p="$(cat "$d/port" 2>/dev/null || true)"; i="$(cat "$d/pid" 2>/dev/null || true)"; case "$i:$p" in (*[!0-9:]*|:*) ;; (*) if [ "$i" -gt 0 ] && [ "$i" -le {MAX_PID} ] && [ "$p" -gt 0 ] && [ "$p" -le 65535 ] && kill -0 "$i" 2>/dev/null; then printf 'state:%s:%s\n' "$i" "$p"; fi;; esac; r="$HOME/.t3/userdata/server-runtime.json"; if [ -r "$r" ]; then printf 'runtime:'; cat "$r"; printf '\n'; fi"#,
        MAX_PID = MAX_PID
    )
}

fn read_metadata(
    target: &ValidatedTarget<'_>,
    key: &str,
    options: &SshConnectOptions,
) -> Result<RemoteServer, SshRouteError> {
    let script = metadata_script(key);
    let args = metadata_args(target, &script);
    let output = run_command(&options.ssh_program, &args, options)?;
    if !output.status.success() {
        return Err(SshRouteError::CommandFailed {
            status: output.status.code(),
            stderr: output.stderr,
        });
    }
    if let Some(server) = parse_state_line(&output.stdout) {
        return Ok(server);
    }
    let runtime = output
        .stdout
        .find("runtime:")
        .map(|index| output.stdout[index + "runtime:".len()..].trim())
        .ok_or(SshRouteError::NoRunningServer)?;
    let server = parse_runtime(runtime)?;
    check_remote_pid(target, server.pid, options)?;
    Ok(server)
}

fn parse_state_line(stdout: &str) -> Option<RemoteServer> {
    stdout.lines().find_map(|line| {
        let mut parts = line.split(':');
        (parts.next() == Some("state")
            && parts
                .next()
                .and_then(|part| part.parse::<u32>().ok())
                .is_some_and(|pid| pid > 0 && pid <= MAX_PID)
            && parts
                .next()
                .and_then(|part| part.parse::<u16>().ok())
                .is_some_and(|port| port > 0))
        .then(|| {
            let mut parts = line.split(':');
            let _ = parts.next();
            RemoteServer {
                pid: parts.next().unwrap().parse().unwrap(),
                port: parts.next().unwrap().parse().unwrap(),
            }
        })
    })
}

fn parse_runtime(runtime: &str) -> Result<RemoteServer, SshRouteError> {
    #[derive(serde::Deserialize)]
    struct Runtime {
        pid: u64,
        port: u64,
        origin: String,
    }
    let runtime: Runtime =
        serde_json::from_str(runtime).map_err(|_| SshRouteError::MalformedMetadata)?;
    let pid = u32::try_from(runtime.pid).map_err(|_| SshRouteError::MalformedMetadata)?;
    let port = u16::try_from(runtime.port).map_err(|_| SshRouteError::MalformedMetadata)?;
    if pid == 0 || pid > MAX_PID || port == 0 {
        return Err(SshRouteError::MalformedMetadata);
    }
    let origin = Url::parse(&runtime.origin).map_err(|_| SshRouteError::MalformedMetadata)?;
    let loopback_origin = match origin.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    if origin.scheme() != "http" || !loopback_origin || origin.port_or_known_default() != Some(port)
    {
        return Err(SshRouteError::MalformedMetadata);
    }
    Ok(RemoteServer { pid, port })
}

fn check_remote_pid(
    target: &ValidatedTarget<'_>,
    pid: u32,
    options: &SshConnectOptions,
) -> Result<(), SshRouteError> {
    let mut args = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
    ];
    if let Some(port) = target.raw.port {
        args.extend(["-p".into(), port.to_string()]);
    }
    args.extend([
        "--".into(),
        target.host_spec.clone(),
        format!("kill -0 {pid}"),
    ]);
    let output = run_command(&options.ssh_program, &args, options)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(SshRouteError::NoRunningServer)
    }
}

fn metadata_args(target: &ValidatedTarget<'_>, script: &str) -> Vec<String> {
    let mut args = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
    ];
    if let Some(port) = target.raw.port {
        args.extend(["-p".into(), port.to_string()]);
    }
    let quoted = script.replace('\'', "'\\''");
    args.extend([
        "--".into(),
        target.host_spec.clone(),
        format!("sh -c '{quoted}'"),
    ]);
    args
}

fn tunnel_args(target: &ValidatedTarget<'_>, local_port: u16, remote_port: u16) -> Vec<String> {
    let mut args = vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "ExitOnForwardFailure=yes".into(),
        "-o".into(),
        "ControlMaster=no".into(),
        "-o".into(),
        "ControlPath=none".into(),
        "-o".into(),
        "ControlPersist=no".into(),
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-v".into(),
        "-n".into(),
        "-N".into(),
        "-L".into(),
        format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"),
        "--".into(),
        target.host_spec.clone(),
    ];
    if let Some(port) = target.raw.port {
        args.splice(2..2, ["-p".into(), port.to_string()]);
    }
    args
}

struct CommandOutput {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

fn spawn_ssh(program: &PathBuf, args: &[String]) -> Result<Child, SshRouteError> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SshRouteError::Spawn(error.to_string()))
}

fn run_command(
    program: &PathBuf,
    args: &[String],
    options: &SshConnectOptions,
) -> Result<CommandOutput, SshRouteError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SshRouteError::Spawn(error.to_string()))?;
    let stdout_rx = spawn_bounded_reader(child.stdout.take(), options.max_output_bytes);
    let stderr_rx = spawn_bounded_reader(child.stderr.take(), options.max_output_bytes);
    let deadline = Instant::now() + options.command_timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SshRouteError::CommandTimeout);
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SshRouteError::Io(error.to_string()));
            }
        }
    };
    let stdout = receive_output(stdout_rx, options.max_output_bytes)?;
    let stderr = receive_output(stderr_rx, options.max_output_bytes)?;
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
}

fn spawn_tunnel_reader(
    pipe: Option<impl Read + Send + 'static>,
    limit: usize,
) -> mpsc::Receiver<Result<String, SshRouteError>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let Some(pipe) = pipe else { return };
        let mut reader = BufReader::new(pipe);
        let mut total = 0;
        loop {
            let mut line = Vec::new();
            match reader
                .by_ref()
                .take((limit.saturating_sub(total) + 1) as u64)
                .read_until(b'\n', &mut line)
            {
                Ok(0) => break,
                Ok(size) => {
                    total += size;
                    if total > limit {
                        let _ = sender.send(Err(SshRouteError::OutputLimit));
                        let _ = std::io::copy(&mut reader, &mut std::io::sink());
                        break;
                    }
                    let _ = sender.send(Ok(String::from_utf8_lossy(&line).into_owned()));
                }
                Err(error) => {
                    let _ = sender.send(Err(SshRouteError::Io(error.to_string())));
                    break;
                }
            }
        }
    });
    receiver
}

fn spawn_bounded_reader(
    pipe: Option<impl Read + Send + 'static>,
    limit: usize,
) -> Option<mpsc::Receiver<Result<Vec<u8>, SshRouteError>>> {
    let pipe = pipe?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut limited = pipe.take((limit + 1) as u64);
        let result = limited
            .read_to_end(&mut bytes)
            .map_err(|error| SshRouteError::Io(error.to_string()));
        let result = result.and_then(|_| {
            if bytes.len() > limit {
                Err(SshRouteError::OutputLimit)
            } else {
                Ok(bytes)
            }
        });
        let _ = sender.send(result);
    });
    Some(receiver)
}

fn receive_output(
    receiver: Option<mpsc::Receiver<Result<Vec<u8>, SshRouteError>>>,
    limit: usize,
) -> Result<String, SshRouteError> {
    let bytes = receiver
        .ok_or_else(|| SshRouteError::Io("SSH child did not expose output.".into()))?
        .recv_timeout(Duration::from_secs(1))
        .map_err(|_| SshRouteError::CommandTimeout)??;
    String::from_utf8(bytes)
        .map_err(|_| SshRouteError::MalformedMetadata)
        .and_then(|text| {
            if text.len() <= limit {
                Ok(text)
            } else {
                Err(SshRouteError::OutputLimit)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn target() -> SshTarget {
        SshTarget {
            alias: "box".into(),
            hostname: "host.example".into(),
            username: Some("julius".into()),
            port: Some(2222),
        }
    }

    fn fake_ssh(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("ssh-fake");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn options(program: PathBuf) -> SshConnectOptions {
        SshConnectOptions {
            ssh_program: program,
            command_timeout: Duration::from_millis(250),
            tunnel_start_timeout: Duration::from_millis(250),
            ..Default::default()
        }
    }

    #[test]
    fn state_key_matches_upstream_shape() {
        assert_eq!(state_key(&target()), "5c31f39e01e39da0");
    }

    #[test]
    fn malformed_runtime_and_missing_server_are_distinct() {
        assert_eq!(parse_runtime("{}"), Err(SshRouteError::MalformedMetadata));
        assert_eq!(parse_state_line("state:nope:42"), None);
        assert_eq!(parse_state_line("state:0:42"), None);
        assert_eq!(
            parse_runtime(r#"{"pid":42,"port":3773,"origin":"http://127.0.0.1:3773"}"#),
            Ok(RemoteServer {
                pid: 42,
                port: 3773
            })
        );
    }

    #[test]
    fn fake_ssh_reports_success_and_drop_reaps_owned_tunnel() {
        let _serial = serial();
        let dir = temp_dir();
        let marker = dir.join("pid");
        let program = dir.join("ssh-fake");
        fs::write(
            &program,
            format!(
                r##"#!/usr/bin/python3
import os, socket, sys, time
if '-L' not in sys.argv:
 print('state:123:3773'); sys.exit(0)
open({marker:?}, 'w').write(str(os.getpid()))
forward = sys.argv[sys.argv.index('-L') + 1].split(':')
assert forward[0] == '127.0.0.1'
time.sleep(0.08)
s = socket.socket(); s.bind((forward[0], int(forward[1]))); s.listen()
print('debug1: Local forwarding listening on 127.0.0.1 port ' + forward[1] + '.', file=sys.stderr, flush=True)
while True: time.sleep(1)
"##,
                marker = marker.to_string_lossy().to_string()
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
        let tunnel = connect(&target(), &options(program)).unwrap();
        assert_eq!(tunnel.local_addr().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        drop(tunnel);
        let pid = fs::read_to_string(marker).unwrap();
        assert!(!Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn tunnel_without_listener_times_out_and_is_reaped() {
        let _serial = serial();
        let dir = temp_dir();
        let marker = dir.join("pid");
        let program = fake_ssh(&dir, &format!(
            "if [ \"$#\" -lt 15 ]; then printf 'state:123:3773\\n'; exit 0; fi; echo $$ > {}; exec sleep 5",
            marker.display()
        ));
        assert!(matches!(
            connect(&target(), &options(program)),
            Err(SshRouteError::TunnelTimeout)
        ));
        let pid = fs::read_to_string(marker).unwrap();
        assert!(!Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn tunnel_exit_is_reported() {
        let _serial = serial();
        let dir = temp_dir();
        let program = fake_ssh(&dir, "if [ \"$#\" -lt 15 ]; then printf 'state:123:3773\\n'; exit 0; fi; echo failed >&2; exit 7");
        assert!(matches!(
            connect(&target(), &options(program)),
            Err(SshRouteError::TunnelExited {
                status: Some(7),
                ..
            })
        ));
    }

    #[test]
    fn fake_ssh_timeout_is_bounded() {
        let _serial = serial();
        let dir = temp_dir();
        let program = fake_ssh(&dir, "sleep 5");
        let error = match connect(&target(), &options(program)) {
            Ok(_) => panic!("expected timeout"),
            Err(error) => error,
        };
        assert_eq!(error, SshRouteError::CommandTimeout);
    }

    #[test]
    fn fake_ssh_with_no_metadata_reports_no_running_server() {
        let _serial = serial();
        let dir = temp_dir();
        let program = fake_ssh(&dir, "exit 0");
        let error = match connect(&target(), &options(program)) {
            Ok(_) => panic!("expected no running server"),
            Err(error) => error,
        };
        assert_eq!(error, SshRouteError::NoRunningServer);
    }

    #[test]
    fn unsupported_target_is_rejected_before_spawn() {
        let _serial = serial();
        let dir = temp_dir();
        let program = fake_ssh(&dir, "exit 0");
        let mut bad = target();
        bad.alias = "bad;host".into();
        assert!(matches!(
            connect(&bad, &options(program)),
            Err(SshRouteError::InvalidTarget)
        ));
    }

    use crate::credentials::MemoryCredentialStore;
    use std::{
        io::Write,
        net::TcpListener as StdListener,
        sync::{Arc, Mutex},
    };

    /// Serves descriptor and token requests; records whether any request
    /// carried credentials.
    fn http_server(environment: &'static str) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
        let listener = StdListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut head = Vec::new();
                let mut byte = [0];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|v| v.parse().ok())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                let _ = stream.read_exact(&mut body);
                log.lock()
                    .unwrap()
                    .push(format!("{head}{}", String::from_utf8_lossy(&body)));
                let reply = if head.starts_with("GET ") {
                    format!(
                        r#"{{"environmentId":"{environment}","label":"T","platform":{{"os":"linux","arch":"x64"}},"serverVersion":"1","capabilities":{{}}}}"#
                    )
                } else {
                    r#"{"access_token":"synthetic-token","token_type":"Bearer","expires_in":3600,"scope":"orchestration:read diagnostics:read"}"#.to_owned()
                };
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", reply.len(), reply);
            }
        });
        (addr, seen)
    }

    fn detached(addr: SocketAddr, target: &SshTarget) -> SshTunnel {
        SshTunnel {
            local_addr: addr,
            child: None,
            identity: target_identity(target),
        }
    }

    #[tokio::test]
    async fn wrong_descriptor_is_rejected_before_any_credential_use() {
        let (addr, seen) = http_server("other");
        let tunnel = detached(addr, &target());
        let result = tunnel
            .verified_endpoint(&EnvironmentId::new("expected").unwrap())
            .await;
        assert!(matches!(result, Err(ConnectionError::WrongEnvironment)));
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(!seen[0].to_ascii_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn credential_scope_is_stable_across_ports_and_isolated_by_target_and_environment() {
        let env = EnvironmentId::new("env-a").unwrap();
        let (addr1, _) = http_server("env-a");
        let (addr2, _) = http_server("env-a");
        assert_ne!(addr1.port(), addr2.port());
        let first = detached(addr1, &target())
            .verified_endpoint(&env)
            .await
            .unwrap();
        let second = detached(addr2, &target())
            .verified_endpoint(&env)
            .await
            .unwrap();
        assert_eq!(
            first.credential_scope().key(),
            second.credential_scope().key()
        );

        let mut other_user = target();
        other_user.username = None;
        let mut other_port = target();
        other_port.port = Some(2223);
        let mut other_alias = target();
        other_alias.alias = "box2".into();
        for changed in [other_user, other_port, other_alias] {
            let endpoint = detached(addr2, &changed)
                .verified_endpoint(&env)
                .await
                .unwrap();
            assert_ne!(
                endpoint.credential_scope().key(),
                first.credential_scope().key()
            );
        }
        let (addr3, _) = http_server("env-b");
        let other_env = detached(addr3, &target())
            .verified_endpoint(&EnvironmentId::new("env-b").unwrap())
            .await
            .unwrap();
        assert_ne!(
            other_env.credential_scope().key(),
            first.credential_scope().key()
        );
        let direct = VerifiedEndpoint::discover(&format!("http://{addr1}/"), &env)
            .await
            .unwrap();
        assert_ne!(
            direct.credential_scope().key(),
            first.credential_scope().key()
        );
    }

    #[tokio::test]
    async fn saved_grant_restores_on_new_port_bound_to_current_origin_only() {
        let env = EnvironmentId::new("env-a").unwrap();
        let store = MemoryCredentialStore::default();
        let (addr1, _) = http_server("env-a");
        let (addr2, _) = http_server("env-a");
        let first = detached(addr1, &target())
            .verified_endpoint(&env)
            .await
            .unwrap();
        let paired = first.pair_and_save("ABCD2345EFGH", &store).await.unwrap();
        assert_eq!(paired.origin, first.base.as_str());

        let second = detached(addr2, &target())
            .verified_endpoint(&env)
            .await
            .unwrap();
        let restored = second
            .load_saved_grant(&store)
            .unwrap()
            .expect("same target restores");
        assert_eq!(restored.origin, second.base.as_str());
        assert_ne!(restored.origin, first.base.as_str());
        assert!(second.websocket_url(&restored, "t").is_ok());
        assert!(first.websocket_url(&restored, "t").is_err());
        assert!(matches!(
            first.websocket_ticket(&restored).await,
            Err(ConnectionError::InvalidRoute)
        ));

        assert!(matches!(
            second.save_grant(&paired, &store),
            Err(ConnectionError::Storage)
        ));
        let direct = VerifiedEndpoint::discover(&format!("http://{addr2}/"), &env)
            .await
            .unwrap();
        assert!(direct.load_saved_grant(&store).unwrap().is_none());
        let mut other = target();
        other.hostname = "other.example".into();
        let other = detached(addr2, &other)
            .verified_endpoint(&env)
            .await
            .unwrap();
        assert!(other.load_saved_grant(&store).unwrap().is_none());
        second.disconnect(&store).unwrap();
        assert!(first.load_saved_grant(&store).unwrap().is_none());
    }

    #[test]
    fn running_state_follows_owned_child() {
        let _serial = serial();
        let dir = temp_dir();
        let program = fake_ssh(&dir, "exec sleep 5");
        let child = spawn_ssh(&program, &[]).unwrap();
        let mut tunnel = SshTunnel {
            local_addr: "127.0.0.1:9".parse().unwrap(),
            child: Some(child),
            identity: String::new(),
        };
        assert!(tunnel.is_running());
        tunnel.child.as_mut().unwrap().kill().unwrap();
        thread::sleep(Duration::from_millis(50));
        assert!(!tunnel.is_running());
    }

    /// A script that is still open for writing in a forking sibling thread
    /// fails to exec with ETXTBSY, so fake-executable tests run one at a time.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "t3-bar-ssh-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
