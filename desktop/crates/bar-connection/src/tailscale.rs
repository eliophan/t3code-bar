use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Read,
    net::IpAddr,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Status {
    backend_state: String,
    #[serde(default)]
    peer: HashMap<String, Peer>,
}
#[derive(Deserialize)]
struct Peer {
    #[serde(rename = "TailscaleIPs", default)]
    ips: Vec<IpAddr>,
}

fn is_tailnet_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => u32::from(ip) & 0xffc00000 == 0x64400000,
        IpAddr::V6(ip) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

fn status_has_peer(bytes: &[u8], ip: IpAddr) -> bool {
    is_tailnet_address(ip)
        && serde_json::from_slice::<Status>(bytes).is_ok_and(|status| {
            status.backend_state == "Running"
                && status.peer.values().any(|peer| peer.ips.contains(&ip))
        })
}

/// HTTP is encrypted by the local Tailscale daemon only for a known tailnet peer.
/// A private or CGNAT address alone is not proof of that transport.
pub(crate) fn is_verified_peer(ip: IpAddr) -> bool {
    if !is_tailnet_address(ip) {
        return false;
    }
    let Some(binary) = [
        "/usr/bin/tailscale",
        "/opt/homebrew/bin/tailscale",
        "/usr/local/bin/tailscale",
    ]
    .into_iter()
    .find(|path| std::path::Path::new(path).is_file()) else {
        return false;
    };
    let Ok(mut child) = Command::new(binary)
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return false;
    };
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    success
        && receiver
            .recv_timeout(Duration::from_millis(100))
            .ok()
            .and_then(Result::ok)
            .is_some_and(|bytes| bytes.len() <= 4 * 1024 * 1024 && status_has_peer(&bytes, ip))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_known_peers_on_a_running_tailnet_are_trusted() {
        let status = br#"{"BackendState":"Running","Peer":{"fixture":{"TailscaleIPs":["100.90.10.20","fd7a:115c:a1e0::1234","192.168.1.6"]}}}"#;
        assert!(status_has_peer(status, "100.90.10.20".parse().unwrap()));
        assert!(status_has_peer(
            status,
            "fd7a:115c:a1e0::1234".parse().unwrap()
        ));
        for ip in [
            "100.90.10.21",
            "192.168.1.6",
            "100.128.0.1",
            "127.0.0.1",
            "fd7a:115c:a1e1::1234",
        ] {
            assert!(!status_has_peer(status, ip.parse().unwrap()));
        }
        assert!(!status_has_peer(
            br#"{"BackendState":"Stopped","Peer":{"fixture":{"TailscaleIPs":["100.90.10.20"]}}}"#,
            "100.90.10.20".parse().unwrap()
        ));
        assert!(!status_has_peer(
            b"invalid",
            "100.90.10.20".parse().unwrap()
        ));
    }
}
