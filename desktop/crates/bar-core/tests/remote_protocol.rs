use bar_core::{
    connection::VerifiedEndpoint,
    remote::{RemoteConnection, SourceStamp, SourceSupervisor},
    usage::UsageQuery,
};
use bar_identity::{EnvironmentId, EnvironmentSelection};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[derive(Clone, Copy)]
enum Mode {
    Normal,
    Denied,
    SilentOnce,
}
#[derive(Default)]
struct Observed {
    requests: Vec<Value>,
    interrupts: Vec<u64>,
    sockets: usize,
    closed: usize,
}
struct Server {
    addr: std::net::SocketAddr,
    state: Arc<Mutex<Observed>>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server(mode: Mode) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = Arc::new(Mutex::new(Observed::default()));
    let shared = state.clone();
    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let state = shared.clone();
            tokio::spawn(async move {
                let mut head = [0; 128];
                let n = stream.peek(&mut head).await.unwrap();
                if head[..n].starts_with(b"GET /ws?") {
                    websocket(stream, mode, state).await;
                } else {
                    http(stream).await;
                }
            });
        }
    });
    Server { addr, state, task }
}
async fn http(mut stream: TcpStream) {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        head.push(stream.read_u8().await.unwrap());
    }
    let head = String::from_utf8(head).unwrap();
    let len = head
        .lines()
        .find_map(|line| {
            line.to_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|n| n.parse::<usize>().ok())
        })
        .unwrap_or(0);
    let mut body = vec![0; len];
    stream.read_exact(&mut body).await.unwrap();
    let value = if head.starts_with("GET ") {
        json!({"environmentId":"remote","label":"Remote","platform":{"os":"linux","arch":"x64","machine":"mac-mini"},"serverVersion":"1","capabilities":{}})
    } else if head.starts_with("POST /oauth/token ") {
        json!({"access_token":"synthetic-token","token_type":"Bearer","expires_in":3600,"scope":"orchestration:read diagnostics:read"})
    } else {
        json!({"ticket":"synthetic-ticket","expiresAt":"2099-01-01T00:00:00Z"})
    };
    let body = value.to_string();
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
}
fn shell_thread(id: &str, visible: Value) -> Value {
    json!({"id":id,"projectId":"p","title":"Remote thread","providerInstanceId":"codex","modelSelection":{"instanceId":"codex","model":"gpt-5.4"},"runtimeMode":"full-access","interactionMode":"default","branch":"feature","worktreePath":"/work","activeProviderThreadId":null,"lineage":{"rootThreadId":id,"parentThreadId":null,"relationshipToParent":null},"forkedFrom":null,"createdBy":"user","creationSource":"mobile","latestRunId":"run","activeRunId":"run","status":"running","pendingRuntimeRequest":null,"latestVisibleMessage":visible,"latestUserMessageAt":null,"hasActionableProposedPlan":false,"itemCount":1,"visibleItemCount":1,"createdAt":"2026-10-01T00:00:00Z","updatedAt":"2026-10-01T00:00:00Z","archivedAt":null,"settledOverride":null,"settledAt":null,"deletedAt":null})
}
fn limits() -> Value {
    json!({"providers":[{"instanceId":"a","driver":"codex","enabled":true,"installed":true,"auth":{"status":"authenticated","email":"u@example.invalid"},"usageLimits":{"checkedAt":"2026-09-03T12:00:00Z","windows":[{"id":"slot","kind":"session","label":"Session","usedPercent":20}]}}],"usageLimitSources":[{"id":"hub","label":"Hub","accounts":[{"id":"h","driver":"codex","email":"u@example.invalid","usageLimits":{"checkedAt":"2026-09-03T12:01:00Z","windows":[{"id":"slot","kind":"session","label":"Session","usedPercent":40}]}}]}]})
}
async fn websocket(stream: TcpStream, mode: Mode, state: Arc<Mutex<Observed>>) {
    let socket_number = {
        let mut s = state.lock().unwrap();
        s.sockets += 1;
        s.sockets
    };
    let mut ws = accept_async(stream).await.unwrap();
    while let Some(Ok(message)) = ws.next().await {
        if message.is_close() {
            break;
        }
        let Ok(text) = message.to_text() else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if v["_tag"] == "Interrupt" {
            state
                .lock()
                .unwrap()
                .interrupts
                .push(v["requestId"].as_u64().unwrap());
            continue;
        }
        if v["_tag"] != "Request" {
            continue;
        }
        state.lock().unwrap().requests.push(v.clone());
        let id = &v["id"];
        let tag = v["tag"].as_str().unwrap();
        if matches!(mode, Mode::SilentOnce)
            && socket_number == 1
            && (tag == "server.getConfig" || tag == "subscribeServerConfig")
        {
            continue;
        }
        let response = match tag {
            "orchestration.subscribeShell" => {
                json!({"_tag":"Chunk","requestId":id,"values":[{"kind":"snapshot","snapshot":{"schemaVersion":1,"snapshotSequence":1,"projects":[{"id":"p","title":"Project name","workspaceRoot":"/repo","defaultModelSelection":null,"faviconPath":"/repo/icon.png","projectIcon":{"kind":"emoji","emoji":"🚀"},"scripts":[],"createdAt":"2026-10-01T00:00:00Z","updatedAt":"2026-10-01T00:00:00Z"}],"threads":[shell_thread("projection", Value::Null),shell_thread("visible",json!({"id":"m","role":"assistant","text":"Shell preview","updatedAt":"2026-10-01T00:02:00Z"}))],"archivedThreads":[]}}]})
            }
            "server.getConfig" => {
                json!({"_tag":"Exit","requestId":id,"exit":{"_tag":"Success","value":limits()}})
            }
            "subscribeServerConfig" if matches!(mode, Mode::Denied) => {
                tokio::time::sleep(Duration::from_millis(30)).await;
                json!({"_tag":"Exit","requestId":id,"exit":{"_tag":"Failure","cause":[{"_tag":"Fail","error":{"_tag":"EnvironmentAuthorizationError","requiredScope":"diagnostics:read"}}]}})
            }
            "subscribeServerConfig" => {
                json!({"_tag":"Chunk","requestId":id,"values":[{"version":1,"type":"snapshot","config":limits()}]})
            }
            "orchestration.getThreadProjection" => {
                json!({"_tag":"Exit","requestId":id,"exit":{"_tag":"Success","value":{"thread":shell_thread("projection",Value::Null),"runs":[],"attempts":[],"nodes":[],"subagents":[],"providerSessions":[],"providerThreads":[],"providerTurns":[],"runtimeRequests":[],"messages":[{"id":"m","threadId":"projection","runId":"run","nodeId":null,"createdBy":"user","creationSource":"mobile","attachments":[],"streaming":false,"role":"assistant","text":"Projection preview","createdAt":"2026-10-01T00:01:00Z","updatedAt":"2026-10-01T00:02:00Z"}],"plans":[],"turnItems":[],"checkpointScopes":[],"checkpoints":[],"contextHandoffs":[],"contextTransfers":[],"visibleTurnItems":[],"updatedAt":"2026-10-01T00:02:00Z"}}})
            }
            "server.getUsageSummary" => {
                json!({"_tag":"Exit","requestId":id,"exit":{"_tag":"Success","value":{"contractVersion":6,"readAt":"2026-01-02T12:00:00Z","timeZone":"UTC","sinceDay":"2026-01-01","untilDay":"2026-01-02","resolution":"hour","sinceTime":"2026-01-01T12:00:00Z","untilTime":"2026-01-02T12:00:00Z","modelFilter":v["payload"]["modelFilter"],"buckets":[{"day":"2026-01-01","hourStart":"2026-01-01T12:00:00Z","provider":"codex","model":"gpt-5.4","sourcePath":"/synthetic","totals":{"uncachedInputTokens":1,"cachedInputTokens":0,"cacheCreationTokens":0,"outputTokens":0,"reasoningTokens":0},"costUsd":1.0,"cacheSavingsUsd":0.0,"costSource":"modelPriced","records":1,"unpricedRecords":0,"sessions":1}],"sources":[{"fingerprint":{"hostId":"host","provider":"codex","resolvedHomePath":"/synthetic","volumeId":"vol"},"status":"ok","scannedFiles":1,"skippedFiles":0,"malformedRecords":0,"distinctSessions":1,"message":null}],"pricing":{"status":"fresh","source":"test","fetchedAt":null,"knownModels":1},"scanDurationMs":1}}})
            }
            _ => panic!("unexpected request {v}"),
        };
        if ws
            .send(Message::Text(response.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
    state.lock().unwrap().closed += 1;
}
async fn supervisor(server: &Server) -> SourceSupervisor {
    let id = EnvironmentId::new("remote").unwrap();
    let endpoint = VerifiedEndpoint::discover(&format!("http://{}/", server.addr), &id)
        .await
        .unwrap();
    let grant = endpoint.pair("ABCD2345EFGH").await.unwrap();
    SourceSupervisor::new(
        RemoteConnection::Direct { endpoint, grant },
        SourceStamp {
            environment: id,
            generation: 1,
            principal: "test".into(),
        },
        "remote",
    )
}
#[tokio::test]
async fn upstream_snapshot_projection_usage_limits_and_repeated_reads() {
    let server = server(Mode::Normal).await;
    let mut source = supervisor(&server).await;
    let filter = json!({"include":[{"provider":"codex","model":"gpt-5.4"}]});
    let query = UsageQuery {
        since_day: "2026-01-01".into(),
        until_day: "2026-01-02".into(),
        time_zone: "UTC".into(),
        resolution: "hour".into(),
        since_time: Some("2026-01-01T12:00:00Z".into()),
        until_time: Some("2026-01-02T12:00:00Z".into()),
        model_filter: Some(serde_json::from_value(filter.clone()).unwrap()),
    };
    for i in 0..140 {
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            source.read((i == 0).then_some(&query)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.threads.len(), 2);
        let row = result
            .threads
            .iter()
            .find(|t| t.id == "projection")
            .unwrap();
        assert_eq!(row.project_title, "Project name");
        assert_eq!(row.project_id, "p");
        assert_eq!(row.workspace_root.as_deref(), Some("/repo"));
        assert_eq!(row.project_favicon_path.as_deref(), Some("/repo/icon.png"));
        assert_eq!(
            serde_json::from_str::<Value>(row.project_icon_json.as_deref().unwrap()).unwrap(),
            json!({"kind":"emoji","emoji":"🚀"})
        );
        assert_eq!(row.status, bar_core::threads::ThreadStatus::Working);
        assert!(row.is_running);
        assert_eq!(row.branch.as_deref(), Some("feature"));
        assert_eq!(row.model_name.as_deref(), Some("gpt-5.4"));
        assert_eq!(result.previews["projection"].text, "Projection preview");
        assert_eq!(result.previews["visible"].text, "Shell preview");
        let merged = bar_core::limits::merge_contributions(
            &[result.limits.unwrap()],
            &EnvironmentSelection::All,
        );
        assert_eq!(merged.accounts[0].windows[0].used_percent, 40.0);
        if i == 0 {
            let contribution = result.usage.unwrap();
            assert_eq!(contribution.query(), &query);
            let usage = bar_core::usage::merge_usage(
                &[contribution],
                &query,
                &EnvironmentSelection::All,
                6,
            );
            assert_eq!(usage.buckets.len(), 1);
            assert_eq!(
                usage.buckets[0].hour_start.as_deref(),
                Some("2026-01-01T12:00:00Z")
            );
            assert_eq!(usage.buckets[0].cost_usd, 1.0);
        }
    }
    let state = server.state.lock().unwrap();
    assert_eq!(state.sockets, 1);
    let payload = &state
        .requests
        .iter()
        .find(|v| v["tag"] == "server.getUsageSummary")
        .unwrap()["payload"];
    assert_eq!(
        *payload,
        json!({"sinceDay":"2026-01-01","untilDay":"2026-01-02","timeZone":"UTC","resolution":"hour","sinceTime":"2026-01-01T12:00:00Z","untilTime":"2026-01-02T12:00:00Z","modelFilter":filter})
    );
    assert_eq!(
        state
            .requests
            .iter()
            .filter(|v| v["tag"] == "orchestration.getThreadProjection")
            .count(),
        140
    );
}
#[tokio::test]
async fn delayed_config_denial_survives_unary_response() {
    let server = server(Mode::Denied).await;
    let mut source = supervisor(&server).await;
    for _ in 0..2 {
        assert!(source.read(None).await.unwrap().limits_permission_denied);
    }
}
#[tokio::test]
async fn silent_server_is_bounded_interrupts_and_reconnects() {
    let server = server(Mode::SilentOnce).await;
    let mut source = supervisor(&server).await;
    assert!(
        tokio::time::timeout(Duration::from_secs(18), source.read(None))
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(source.read(None).await.unwrap().threads.len(), 2);
    tokio::time::sleep(Duration::from_millis(30)).await;
    let state = server.state.lock().unwrap();
    assert_eq!(state.sockets, 2);
    assert!(state.closed >= 1);
    let interrupted: HashSet<_> = state.interrupts.iter().copied().collect();
    assert!(state
        .requests
        .iter()
        .filter(|v| v["tag"] == "server.getConfig" || v["tag"] == "subscribeServerConfig")
        .take(2)
        .all(|v| interrupted.contains(&v["id"].as_u64().unwrap())));
}

#[allow(dead_code)]
#[path = "../../bar-connection/examples/remote_fixture_server.rs"]
mod native_fixture;

#[tokio::test]
async fn native_fixture_supports_actual_supervisor_hourly_query() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(native_fixture::handle(stream, "remote-b", "Remote B", 60));
        }
    });
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        let id = EnvironmentId::new("remote-b").unwrap();
        let endpoint = VerifiedEndpoint::discover(&format!("http://{address}/"), &id)
            .await
            .unwrap();
        let grant = endpoint.pair("JKLM2345NPQR").await.unwrap();
        let mut reader = SourceSupervisor::new(
            RemoteConnection::Direct { endpoint, grant },
            SourceStamp {
                environment: id,
                generation: 1,
                principal: "fixture".into(),
            },
            "Remote B",
        );
        let query = UsageQuery {
            since_day: "2026-11-05".into(),
            until_day: "2026-11-06".into(),
            time_zone: "UTC".into(),
            resolution: "hour".into(),
            since_time: Some("2026-11-05T09:08:00.000Z".into()),
            until_time: Some("2026-11-06T09:08:00.000Z".into()),
            model_filter: None,
        };
        let read = reader.read(Some(&query)).await.unwrap();
        assert_eq!(read.threads.len(), 1);
        let usage = read.usage.unwrap();
        assert_eq!(usage.query(), &query);
        let merged = bar_core::usage::merge_usage(
            &[usage],
            &query,
            &EnvironmentSelection::All,
            bar_core::usage::USAGE_CONTRACT_VERSION,
        );
        assert_eq!(merged.buckets.len(), 1);
        assert_eq!(merged.buckets[0].totals.uncached_input_tokens, 60);
    })
    .await;
    task.abort();
    result.unwrap();
}

mod ssh_session {
    use super::*;
    use bar_core::connection::{
        catalog::SshTarget,
        credentials::MemoryCredentialStore,
        ssh::{connect_async, SshConnectOptions},
        ConnectionError,
    };
    use std::{os::unix::fs::PermissionsExt, path::Path};

    /// Fake OpenSSH: answers the metadata probe and, for `-L`, listens on the
    /// requested loopback port and relays to the fixture server named in the
    /// sibling `upstream` file. It records its pid in `pid-<local port>`.
    const FAKE_SSH: &str = r#"#!/usr/bin/python3
import os, socket, sys, threading, time
here = os.path.dirname(os.path.abspath(__file__))
if '-L' not in sys.argv:
    print('state:123:3773'); sys.exit(0)
local = sys.argv[sys.argv.index('-L') + 1].split(':')
upstream = int(open(os.path.join(here, 'upstream')).read())
open(os.path.join(here, 'pid-' + local[1]), 'w').write(str(os.getpid()))
listener = socket.socket(); listener.bind((local[0], int(local[1]))); listener.listen()
print('debug1: Local forwarding listening on 127.0.0.1 port ' + local[1] + '.', file=sys.stderr, flush=True)
def pump(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data: break
            b.sendall(data)
    except OSError: pass
    for s in (a, b):
        try: s.shutdown(socket.SHUT_RDWR)
        except OSError: pass
while True:
    client, _ = listener.accept()
    remote = socket.create_connection(('127.0.0.1', upstream))
    threading.Thread(target=pump, args=(client, remote), daemon=True).start()
    threading.Thread(target=pump, args=(remote, client), daemon=True).start()
"#;

    fn target() -> SshTarget {
        SshTarget {
            alias: "box".into(),
            hostname: "host.example".into(),
            username: Some("user".into()),
            port: None,
        }
    }

    fn options(dir: &Path, upstream: u16) -> SshConnectOptions {
        let program = dir.join("ssh-fake");
        std::fs::write(&program, FAKE_SSH).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(dir.join("upstream"), upstream.to_string()).unwrap();
        SshConnectOptions {
            ssh_program: program,
            command_timeout: Duration::from_secs(5),
            tunnel_start_timeout: Duration::from_secs(5),
            ..Default::default()
        }
    }

    fn alive(dir: &Path, port: u16) -> bool {
        let pid = std::fs::read_to_string(dir.join(format!("pid-{port}"))).unwrap();
        std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    async fn gone(dir: &Path, port: u16) -> bool {
        for _ in 0..100 {
            if !alive(dir, port) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    async fn session(
        tunnel: bar_core::connection::ssh::SshTunnel,
        store: &MemoryCredentialStore,
        code: Option<&str>,
    ) -> Option<SourceSupervisor> {
        let id = EnvironmentId::new("remote").unwrap();
        let endpoint = tunnel.verified_endpoint(&id).await.unwrap();
        let grant = match endpoint.load_saved_grant(store).unwrap() {
            Some(grant) => grant,
            None => endpoint.pair_and_save(code?, store).await.unwrap(),
        };
        Some(SourceSupervisor::new(
            RemoteConnection::Ssh {
                endpoint,
                grant,
                tunnel,
            },
            SourceStamp {
                environment: id,
                generation: 1,
                principal: "ssh".into(),
            },
            "remote",
        ))
    }

    #[tokio::test]
    async fn pairs_polls_restores_on_new_port_and_reaps_tunnels_with_the_session() {
        let server = server(Mode::Normal).await;
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path(), server.addr.port());
        let store = MemoryCredentialStore::default();

        let tunnel = connect_async(target(), options.clone()).await.unwrap();
        let first_port = tunnel.local_addr().port();
        let mut first = session(tunnel, &store, Some("ABCD2345EFGH")).await.unwrap();
        let result = first.read(None).await.unwrap();
        assert_eq!(result.threads.len(), 2);
        assert!(first.usable());
        assert!(alive(dir.path(), first_port));

        let tunnel = connect_async(target(), options.clone()).await.unwrap();
        let second_port = tunnel.local_addr().port();
        assert_ne!(first_port, second_port);
        let mut second = session(tunnel, &store, None)
            .await
            .expect("saved grant restores without a code");
        assert_eq!(second.read(None).await.unwrap().threads.len(), 2);

        drop(first);
        assert!(gone(dir.path(), first_port).await);
        assert!(alive(dir.path(), second_port));
        drop(second);
        assert!(gone(dir.path(), second_port).await);
    }

    #[tokio::test]
    async fn wrong_environment_and_failed_pairing_drop_and_reap_the_tunnel() {
        let server = server(Mode::Normal).await;
        let dir = tempfile::tempdir().unwrap();
        let options = options(dir.path(), server.addr.port());

        let tunnel = connect_async(target(), options.clone()).await.unwrap();
        let port = tunnel.local_addr().port();
        let result = tunnel
            .verified_endpoint(&EnvironmentId::new("different").unwrap())
            .await;
        assert!(matches!(result, Err(ConnectionError::WrongEnvironment)));
        drop(tunnel);
        assert!(gone(dir.path(), port).await);

        let tunnel = connect_async(target(), options).await.unwrap();
        let port = tunnel.local_addr().port();
        let id = EnvironmentId::new("remote").unwrap();
        let endpoint = tunnel.verified_endpoint(&id).await.unwrap();
        assert!(matches!(
            endpoint.pair("short").await,
            Err(ConnectionError::PairingRejected)
        ));
        drop((endpoint, tunnel));
        assert!(gone(dir.path(), port).await);
    }

    #[tokio::test]
    async fn supervisor_is_unusable_after_its_tunnel_exits() {
        let server = server(Mode::Normal).await;
        let dir = tempfile::tempdir().unwrap();
        let tunnel = connect_async(target(), options(dir.path(), server.addr.port()))
            .await
            .unwrap();
        let port = tunnel.local_addr().port();
        let store = MemoryCredentialStore::default();
        let mut source = session(tunnel, &store, Some("ABCD2345EFGH")).await.unwrap();
        assert!(source.usable());
        let pid = std::fs::read_to_string(dir.path().join(format!("pid-{port}"))).unwrap();
        std::process::Command::new("kill")
            .args(["-9", pid.trim()])
            .status()
            .unwrap();
        for _ in 0..100 {
            if !source.usable() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("supervisor stayed usable after its tunnel died");
    }
}
