use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicUsize, Ordering},
};

#[test]
fn local_pairing_links_accept_only_the_configured_loopback_destination() {
    let port = 3773;
    for address in ["127.0.0.1", "localhost", "[::1]"] {
        let link = format!(" http://{address}:{port}/pair#token=abcd2345efgh ");
        assert_eq!(local_pairing_input(port, &link).unwrap(), "ABCD2345EFGH");
    }
    let direct = "http://127.0.0.1:3773/pair#token=ABCD2345EFGH";
    let desktop = format!(
        "t3code://pair?pairingUrl={}",
        url::form_urlencoded::byte_serialize(direct.as_bytes()).collect::<String>()
    );
    let hosted = "https://app.t3.codes/pair?host=http%3A%2F%2F127.0.0.1%3A3773#token=ABCD2345EFGH";
    for link in [&desktop, hosted] {
        assert_eq!(local_pairing_input(port, link).unwrap(), "ABCD2345EFGH");
    }
    for input in ["ABCD2345EFGH", "existing-access-token"] {
        assert_eq!(local_pairing_input(port, input).unwrap(), input);
    }
    for link in [
        "http://127.0.0.1:3774/pair#token=ABCD2345EFGH",
        "https://127.0.0.1:3773/pair#token=ABCD2345EFGH",
        "https://remote.example/pair#token=ABCD2345EFGH",
        "http://localhost.example:3773/pair#token=ABCD2345EFGH",
        "https://app.t3.codes/pair?host=https%3A%2F%2Fremote.example#token=ABCD2345EFGH",
    ] {
        assert_eq!(
            local_pairing_input(port, link),
            Err(ConnectionError::PairingDestination)
        );
    }
}

#[test]
fn malformed_pairing_links_are_rejected_without_echoing_the_code() {
    for link in [
        "http://127.0.0.1:3773/pair",
        "http://127.0.0.1:3773/pair?token=ABCD2345EFGH",
        "http://127.0.0.1:3773/pair#token=ABCD2345EFGH&token=ABCD2345EFGH",
        "http://127.0.0.1:3773/pair#token=invalid",
        "http://127.0.0.1:3773/oauth/token#token=ABCD2345EFGH",
        "http://user:password@127.0.0.1:3773/pair#token=ABCD2345EFGH",
        "t3code://pair?pairingUrl=invalid",
        "t3code://pair?pairingUrl=one&pairingUrl=two",
        "https://app.t3.codes/pair?host=http%3A%2F%2F127.0.0.1%3A3773&host=other#token=ABCD2345EFGH",
        "https://app.t3.codes/pair?host=http%3A%2F%2F127.0.0.1%3A3773%2Fother#token=ABCD2345EFGH",
        "http://",
    ] {
        let error = local_pairing_input(3773, link).unwrap_err();
        assert_eq!(error, ConnectionError::InvalidPairingLink);
        assert!(!error.to_string().contains("ABCD2345EFGH"));
    }
}

#[test]
fn pairing_code_shape_matches_t3_code_and_bearer_tokens_pass_through() {
    assert!(is_pairing_code("ABCD2345EFGH"));
    assert!(is_pairing_code("abcd2345efgh"));
    assert!(!is_pairing_code("ABCD2345EFG"));
    assert!(!is_pairing_code("ABCD2345EFG0"));
    assert!(!is_pairing_code("eyJhbGciOiJIUzI1NiJ9"));
    runtime().block_on(async {
        let token = LoopbackTransport
            .exchange_pairing(1, "existing-access-token")
            .await
            .unwrap();
        assert_eq!(token, "existing-access-token");
    });
}

#[test]
fn pairing_code_is_exchanged_before_limits_ticket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_head(&mut stream);
        let content_length = request
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let mut form = vec![0; content_length];
        stream.read_exact(&mut form).unwrap();
        let form = String::from_utf8(form).unwrap();
        assert!(request.starts_with("POST /oauth/token HTTP/1.1\r\n"));
        assert!(request.contains("Content-Type: application/x-www-form-urlencoded\r\n"));
        assert!(form.contains("subject_token=ABCD2345EFGH"));
        assert!(form.contains(
            "subject_token_type=urn%3At3%3Aparams%3Aoauth%3Atoken-type%3Aenvironment-bootstrap"
        ));
        assert!(form.contains(
            "requested_token_type=urn%3Aietf%3Aparams%3Aoauth%3Atoken-type%3Aaccess_token"
        ));
        let body = r#"{"access_token":"eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDAwMDAwMDB9.sig","token_type":"Bearer","expires_in":3600,"scope":"orchestration:read"}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    runtime().block_on(async {
        let token = LoopbackTransport
            .exchange_pairing(port, " ABCD2345EFGH ")
            .await
            .unwrap();
        assert!(token.starts_with("eyJhbGciOiJub25lIn0."));
    });
    server.join().unwrap();
}

#[test]
fn pairing_exchange_rejects_invalid_grants_and_unsupported_tokens() {
    for (status, body, expected) in [
        (401, "{}", ConnectionError::PairingRejected),
        (
            200,
            r#"{"access_token":"session-token","token_type":"DPoP","expires_in":3600}"#,
            ConnectionError::Protocol,
        ),
        (
            200,
            r#"{"access_token":"session-token","token_type":"Bearer","expires_in":0}"#,
            ConnectionError::Protocol,
        ),
        (
            200,
            r#"{"token_type":"Bearer","expires_in":3600}"#,
            ConnectionError::Protocol,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_head(&mut stream);
            let length: usize = request
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            let mut form = vec![0; length];
            stream.read_exact(&mut form).unwrap();
            write!(stream, "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        runtime().block_on(async {
            let store = Arc::new(Memory::default());
            let connection = LimitsConnection::with_default_transport(store.clone());
            assert_eq!(
                connection.connect(port, "ABCD2345EFGH").await,
                Err(expected)
            );
            assert!(store.load().unwrap().is_none());
            assert!(!connection.snapshot().has_credential);
        });
        server.join().unwrap();
    }
}

#[derive(Default)]
struct Memory(Mutex<Option<Credential>>);
impl CredentialStore for Memory {
    fn load(&self) -> Result<Option<Credential>, ConnectionError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, c: &Credential) -> Result<(), ConnectionError> {
        *self.0.lock().unwrap() = Some(c.clone());
        Ok(())
    }
    fn delete(&self) -> Result<(), ConnectionError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

struct BrokenStore;
impl CredentialStore for BrokenStore {
    fn load(&self) -> Result<Option<Credential>, ConnectionError> {
        Err(ConnectionError::Storage)
    }
    fn save(&self, _: &Credential) -> Result<(), ConnectionError> {
        Err(ConnectionError::Storage)
    }
    fn delete(&self) -> Result<(), ConnectionError> {
        Err(ConnectionError::Storage)
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
async fn until(mut f: impl FnMut() -> bool) {
    timeout(Duration::from_secs(4), async {
        while !f() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}
fn read_head(s: &mut std::net::TcpStream) -> String {
    let mut b = Vec::new();
    let mut c = [0];
    while !b.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut c).unwrap();
        b.push(c[0]);
    }
    String::from_utf8(b).unwrap()
}
fn client_frame(s: &mut std::net::TcpStream) -> Value {
    let mut h = [0; 2];
    s.read_exact(&mut h).unwrap();
    assert_eq!(h[0], 0x81);
    assert_ne!(h[1] & 128, 0);
    let code = h[1] & 127;
    let len = if code < 126 {
        code as usize
    } else if code == 126 {
        let mut x = [0; 2];
        s.read_exact(&mut x).unwrap();
        u16::from_be_bytes(x) as usize
    } else {
        let mut x = [0; 8];
        s.read_exact(&mut x).unwrap();
        u64::from_be_bytes(x) as usize
    };
    let mut mask = [0; 4];
    s.read_exact(&mut mask).unwrap();
    let mut b = vec![0; len];
    s.read_exact(&mut b).unwrap();
    for (i, v) in b.iter_mut().enumerate() {
        *v ^= mask[i % 4];
    }
    serde_json::from_slice(&b).unwrap()
}
fn write_frame(s: &mut std::net::TcpStream, value: &str) {
    let b = value.as_bytes();
    assert!(b.len() < 126);
    s.write_all(&[0x81, b.len() as u8]).unwrap();
    s.write_all(b).unwrap();
}
#[test]
fn loopback_bootstrap_subscribe_ack_and_snapshot() {
    loopback_connection(false, false);
}

#[test]
fn loopback_pairing_saves_access_token_and_streams_limits() {
    loopback_connection(true, false);
}

#[test]
fn loopback_pairing_link_saves_only_the_exchanged_access_token() {
    loopback_connection(true, true);
}

fn loopback_connection(pairing: bool, link: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if pairing {
            let (mut exchange, _) = listener.accept().unwrap();
            let request = read_head(&mut exchange);
            assert!(request.starts_with("POST /oauth/token HTTP/1.1\r\n"));
            let length: usize = request
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            let mut form = vec![0; length];
            exchange.read_exact(&mut form).unwrap();
            assert!(String::from_utf8(form)
                .unwrap()
                .contains("subject_token=ABCD2345EFGH"));
            let body = r#"{"access_token":"test-token","token_type":"Bearer","expires_in":3600}"#;
            write!(
                exchange,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        let (mut http, _) = listener.accept().unwrap();
        let request = read_head(&mut http);
        assert!(request.starts_with("POST /api/auth/websocket-ticket/rpc HTTP/1.1\r\n"));
        assert!(request.contains("Authorization: Bearer test-token\r\n"));
        let body = r#"{"ticket":"ticket+/=","expiresAt":"2999-01-01T00:00:00Z"}"#;
        write!(
            http,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        let (mut ws, _) = listener.accept().unwrap();
        ws.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let request = read_head(&mut ws);
        assert!(request.starts_with("GET /ws?wsTicket=ticket%2B%2F%3D&orchestrationProtocol=2 "));
        let key = request
            .lines()
            .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
            .unwrap();
        let accept = STANDARD.encode(Sha1::digest(
            format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
        ));
        write!(ws,"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").unwrap();
        assert_eq!(
            client_frame(&mut ws),
            json!({"_tag":"Request","id":0,"tag":"subscribeServerConfig","payload":{"usageLimitSources":true,"workspaceSnapshotsOnDemand":true},"headers":[]})
        );
        write_frame(
            &mut ws,
            r#"{"_tag":"Chunk","requestId":0,"values":[{"version":1,"type":"snapshot","config":{"providers":[]}}]}"#,
        );
        assert_eq!(client_frame(&mut ws), json!({"_tag":"Ack","requestId":0}));
        std::thread::sleep(Duration::from_millis(200));
    });
    runtime().block_on(async {
        let store = Arc::new(Memory::default());
        let c = LimitsConnection::with_default_transport(store.clone());
        let pairing_link = format!("http://127.0.0.1:{port}/pair#token=ABCD2345EFGH");
        c.connect(
            port,
            if link {
                &pairing_link
            } else if pairing {
                "ABCD2345EFGH"
            } else {
                "test-token"
            },
        )
        .await
        .unwrap();
        assert_eq!(store.load().unwrap().unwrap().token, "test-token");
        until(|| c.snapshot().status == LiveStatus::Live).await;
        assert_eq!(
            c.snapshot().data.unwrap().state,
            LimitsState::NoLimitsReported
        );
        assert!(!serde_json::to_string(&c.snapshot())
            .unwrap()
            .contains("test-token"));
        c.disconnect().await.unwrap();
    });
    server.join().unwrap();
}
#[test]
fn stored_credential_encoding_and_redaction() {
    let c: Credential =
        serde_json::from_str(r#"{"port":1234,"token":"private","expiresAt":800000000}"#).unwrap();
    assert_eq!(c.expires_at + SWIFT_EPOCH, 1778307200.0);
    assert_eq!(
        serde_json::to_value(&c).unwrap()["expiresAt"].as_f64(),
        Some(800000000.0)
    );
    assert!(!format!("{c:?}").contains("private"));
    assert_eq!(
        Credential::new(0, "x").err(),
        Some(ConnectionError::Destination)
    );
    assert_eq!(
        Credential::new(1, "x\r\nfoo").err(),
        Some(ConnectionError::InvalidToken)
    );
}
#[test]
fn tauri_credential_namespace_is_independent_of_swift() {
    let store = KeyringStore::production();
    assert_eq!(store.service, "com.t3code.bar.tauri.limits");
    assert_eq!(store.account, "pairing-key");
}
#[test]
fn saved_pairing_is_restored_offline_without_a_transport_request() {
    let store = Arc::new(Memory::default());
    let transport = Arc::new(Mock {
        calls: AtomicUsize::new(0),
        mode: 0,
    });
    let first = LimitsConnection::new(store.clone(), transport.clone());
    first.restore_saved().unwrap();
    assert!(!first.snapshot().has_credential);
    store
        .save(&Credential::new(42, "synthetic-key").unwrap())
        .unwrap();
    let restarted = LimitsConnection::new(store.clone(), transport.clone());
    restarted.restore_saved().unwrap();
    assert!(restarted.snapshot().has_credential);
    assert_eq!(restarted.snapshot().status, LiveStatus::Paused);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    runtime().block_on(restarted.disconnect()).unwrap();
    assert!(!restarted.snapshot().has_credential);
    assert!(store.load().unwrap().is_none());
}

#[test]
fn restore_storage_errors_are_explicit_and_do_not_fake_pairing() {
    let c = LimitsConnection::with_default_transport(Arc::new(BrokenStore));
    assert_eq!(c.restore_saved(), Err(ConnectionError::Storage));
    assert_eq!(c.snapshot().status, LiveStatus::StorageUnavailable);
    assert!(!c.snapshot().has_credential);
}
struct Mock {
    calls: AtomicUsize,
    mode: usize,
}
struct MockSocket {
    index: usize,
    mode: usize,
}
#[async_trait]
impl LiveSocket for MockSocket {
    async fn send(&mut self, _: &str) -> Result<(), ConnectionError> {
        Ok(())
    }
    async fn receive(&mut self) -> Result<String, ConnectionError> {
        self.index += 1;
        if self.index == 1 {
            return Ok(match self.mode { 1 => r#"{"_tag":"Chunk","requestId":0,"values":[{"version":2,"type":"snapshot","config":{"providers":[]}}]}"#.into(), 2 => "x".repeat(MAX_FRAME+1), _ => r#"{"_tag":"Chunk","requestId":0,"values":[{"version":1,"type":"snapshot","config":{"providers":[]}}]}"#.into() });
        }
        if self.mode == 5 {
            std::future::pending::<()>().await;
        }
        Err(ConnectionError::Transport)
    }
}
#[async_trait]
impl LiveTransport for Mock {
    async fn exchange_pairing(&self, _: u16, token: &str) -> Result<String, ConnectionError> {
        if is_pairing_code(token.trim()) {
            return Err(ConnectionError::PairingRejected);
        }
        Ok(token.trim().to_owned())
    }
    async fn ticket(&self, _: u16, _: &str) -> Result<(String, DateTime<Utc>), ConnectionError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.mode == 3 {
            return Err(ConnectionError::Expired);
        }
        if self.mode == 4 {
            std::future::pending::<()>().await;
        }
        if self.mode == 0 && n > 0 {
            std::future::pending::<()>().await;
        }
        Ok(("t".into(), Utc::now() + chrono::Duration::minutes(1)))
    }
    async fn socket(&self, _: u16, _: &str) -> Result<Box<dyn LiveSocket>, ConnectionError> {
        Ok(Box::new(MockSocket {
            index: 0,
            mode: self.mode,
        }))
    }
}
#[test]
fn invalid_replacement_preserves_active_connection_and_saved_key() {
    runtime().block_on(async {
        let store = Arc::new(Memory::default());
        let transport = Arc::new(Mock {
            calls: AtomicUsize::new(0),
            mode: 5,
        });
        let c = LimitsConnection::new(store.clone(), transport.clone());
        c.connect(42, "original-key").await.unwrap();
        until(|| c.snapshot().status == LiveStatus::Live).await;
        let before = c.snapshot();
        let (generation, task_id) = {
            let state = c.inner.lock().unwrap();
            (state.generation, state.task.as_ref().unwrap().id())
        };

        assert_eq!(c.connect(43, "").await, Err(ConnectionError::InvalidToken));
        assert_eq!(
            c.connect(43, "http://127.0.0.1:44/pair#token=ABCD2345EFGH")
                .await,
            Err(ConnectionError::PairingDestination)
        );
        assert_eq!(
            c.connect(43, "http://127.0.0.1:43/pair#token=invalid")
                .await,
            Err(ConnectionError::InvalidPairingLink)
        );
        assert_eq!(
            c.connect(43, "ABCD2345EFGH").await,
            Err(ConnectionError::PairingRejected)
        );
        tokio::task::yield_now().await;

        assert_eq!(c.snapshot(), before);
        {
            let state = c.inner.lock().unwrap();
            assert_eq!(state.generation, generation);
            let task = state.task.as_ref().unwrap();
            assert_eq!(task.id(), task_id);
            assert!(!task.is_finished());
            let credential = state.credential.as_ref().unwrap();
            assert_eq!(credential.port, 42);
            assert_eq!(credential.token, "original-key");
        }
        let saved = store.load().unwrap().unwrap();
        assert_eq!(saved.port, 42);
        assert_eq!(saved.token, "original-key");
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
        c.disconnect().await.unwrap();
    });
}
#[test]
fn protocol_failures_and_revocation_are_permanent() {
    runtime().block_on(async {
        for mode in [1, 2, 3] {
            let store = Arc::new(Memory::default());
            let mock = Arc::new(Mock {
                calls: AtomicUsize::new(0),
                mode,
            });
            let c = LimitsConnection::new(store.clone(), mock.clone());
            c.connect(1234, "secret").await.unwrap();
            until(|| matches!(c.snapshot().status, LiveStatus::Failed(_))).await;
            assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
            assert_eq!(store.load().unwrap().is_none(), mode == 3);
            assert!(!c.snapshot().has_credential);
            assert!(!format!("{:?}", c.snapshot()).contains("secret"));
        }
    });
}
#[test]
fn retry_keeps_data_and_disconnect_cancels_pending_run() {
    runtime().block_on(async {
        let store = Arc::new(Memory::default());
        let mock = Arc::new(Mock {
            calls: AtomicUsize::new(0),
            mode: 0,
        });
        let c = LimitsConnection::new(store.clone(), mock.clone())
            .with_retry_delay(Duration::from_millis(10));
        c.connect(42, "secret").await.unwrap();
        until(|| mock.calls.load(Ordering::SeqCst) >= 2).await;
        assert!(c.snapshot().data.is_some());
        assert_eq!(c.snapshot().status, LiveStatus::Retrying);
        c.disconnect().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(c.snapshot().status, LiveStatus::CacheOnly);
        assert!(c.snapshot().data.is_none());
        assert!(store.load().unwrap().is_none());
        c.reconnect_saved_if_available(42).await.unwrap();
        assert_eq!(c.snapshot().status, LiveStatus::CacheOnly);
        assert_eq!(
            c.reconnect_saved(42).await,
            Err(ConnectionError::NoSavedCredential)
        );
        c.connect(42, "new").await.unwrap();
        c.pause().await;
        assert_eq!(c.snapshot().status, LiveStatus::Paused);
        c.resume().await.unwrap();
        c.disconnect().await.unwrap();
    });
}
#[test]
fn audit_chunk_size_never_panics() {
    let result = std::panic::catch_unwind(|| decode_chunks(b"ffffffffffffffed\r\n"));
    assert!(
        result.is_ok(),
        "malformed chunk size must return Protocol rather than panic"
    );
}
struct PongTransport {
    calls: AtomicUsize,
}
struct PongSocket;
#[async_trait]
impl LiveSocket for PongSocket {
    async fn send(&mut self, _: &str) -> Result<(), ConnectionError> {
        Ok(())
    }
    async fn receive(&mut self) -> Result<String, ConnectionError> {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Ok(r#"{"_tag":"Pong"}"#.into())
    }
}
#[async_trait]
impl LiveTransport for PongTransport {
    async fn ticket(&self, _: u16, _: &str) -> Result<(String, DateTime<Utc>), ConnectionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(("t".into(), Utc::now() + chrono::Duration::minutes(5)))
    }
    async fn socket(&self, _: u16, _: &str) -> Result<Box<dyn LiveSocket>, ConnectionError> {
        Ok(Box::new(PongSocket))
    }
}
#[test]
fn audit_snapshot_deadline_is_independent_of_pongs() {
    runtime().block_on(async {
        let transport = Arc::new(PongTransport { calls: AtomicUsize::new(0) });
        let c = LimitsConnection::new(Arc::new(Memory::default()), transport.clone()).with_retry_delay(Duration::from_millis(1));
        c.connect(42, "synthetic").await.unwrap();
        tokio::time::sleep(Duration::from_secs(17)).await;
        let calls = transport.calls.load(Ordering::SeqCst);
        let status = c.snapshot().status;
        c.disconnect().await.unwrap();
        assert!(calls > 1, "expected initial-snapshot watchdog to reconnect, got {calls} ticket call and {status:?}");
    });
}

#[test]
fn audit_malformed_chunk_does_not_strand_connecting() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut http, _) = listener.accept().unwrap();
        read_head(&mut http);
        http.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nffffffffffffffed\r\n").unwrap();
    });
    runtime().block_on(async {
        let c = LimitsConnection::with_default_transport(Arc::new(Memory::default()));
        c.connect(port, "synthetic").await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let snapshot = c.snapshot();
        let task_finished = c
            .inner
            .lock()
            .unwrap()
            .task
            .as_ref()
            .is_some_and(|t| t.is_finished());
        c.disconnect().await.unwrap();
        assert!(
            !task_finished || matches!(snapshot.status, LiveStatus::Failed(_)),
            "task finished but state is {:?}",
            snapshot.status
        );
    });
    server.join().unwrap();
}
