//! Contract-shaped loopback server for the multi-environment Bar fixture.
//! It has no persistence and accepts only synthetic values.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[tokio::main]
async fn main() {
    let ports = [
        (43101, "remote-a", "Remote A", 20),
        (43102, "remote-b", "Remote B", 60),
    ];
    let mut tasks = Vec::new();
    for (port, id, label, used) in ports {
        tasks.push(tokio::spawn(serve(port, id, label, used)));
    }
    let _ = futures_util::future::join_all(tasks).await;
}
async fn serve(port: u16, id: &'static str, label: &'static str, used: i64) {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        tokio::spawn(handle(stream, id, label, used));
    }
}
pub(crate) async fn handle(
    stream: TcpStream,
    id: &'static str,
    label: &'static str,
    used: i64,
) {
    handle_counting_projections(stream, id, label, used, std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0))).await;
}

pub(crate) async fn handle_counting_projections(
    mut stream: TcpStream,
    id: &'static str,
    label: &'static str,
    used: i64,
    projections: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    if offline(id) {
        return;
    }
    let Ok(Ok(line)) =
        tokio::time::timeout(std::time::Duration::from_secs(5), request_line(&stream)).await
    else {
        return;
    };
    if !line.starts_with("GET /ws") && read_http(&mut stream).await.is_err() {
        return;
    }
    if line.starts_with("GET /.well-known/t3/environment") {
        respond(&mut stream, &json!({"environmentId":id,"label":label,"platform":{"os":"linux","arch":"x64","machine":"linux"},"serverVersion":"fixture","capabilities":{}})).await;
        return;
    }
    if line.starts_with("POST /oauth/token") {
        respond(&mut stream, &json!({"access_token":format!("fixture-{id}"),"token_type":"Bearer","expires_in":3600,"scope":"orchestration:read diagnostics:read"})).await;
        return;
    }
    if line.starts_with("POST /api/auth/websocket-ticket/rpc") {
        respond(
            &mut stream,
            &json!({"ticket":"fixture-ticket","expiresAt":"2099-01-01T00:00:00Z"}),
        )
        .await;
        return;
    }
    if line.starts_with("GET /ws") {
        let mut socket = accept_async(stream).await.unwrap();
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            if offline(id) {
                break;
            }
            let request: Value = serde_json::from_str(&text).unwrap();
            if request["_tag"] != "Request" {
                continue;
            }
            let request_id = request["id"].as_u64().unwrap();
            match request["tag"].as_str().unwrap_or_default() {
                "orchestration.subscribeShell" => send_chunk(&mut socket, request_id, json!([{"kind":"snapshot","snapshot":{"schemaVersion":1,"snapshotSequence":1,"projects":[{"id":format!("project-{id}"),"title":label}],"threads":[thread(id, label)],"archivedThreads":[]}}])).await,
                "orchestration.getThreadProjection" => {
                    projections.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    send_success(&mut socket, request_id, json!({"thread":thread(id, label),"messages":[{"role":"assistant","text":format!("Preview for {label}"),"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"}]})).await;
                },
                "server.getConfig" => send_success(&mut socket, request_id, config(id, used)).await,
                "subscribeServerConfig" => send_chunk(&mut socket, request_id, json!([json!({"version":1,"type":"snapshot","config":config(id, used)})])).await,
                "server.getUsageSummary" => send_success(&mut socket, request_id, usage(id, used, &request["payload"])).await,
                _ => {}
            }
        }
    }
}
fn offline(id: &str) -> bool {
    std::env::var_os("T3_BAR_FIXTURE_OFFLINE_FILE")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .is_some_and(|text| text.lines().any(|line| line == id))
}
fn thread(id: &str, label: &str) -> Value {
    json!({"id":"same-thread-id","projectId":format!("project-{id}"),"title":format!("{label} thread"),"providerInstanceId":"codex","modelSelection":{"instanceId":"codex","model":"fixture"},"runtimeMode":"full-access","interactionMode":"default","branch":null,"worktreePath":null,"lineage":{"rootThreadId":"same-thread-id","parentThreadId":null,"relationshipToParent":null},"forkedFrom":null,"activeProviderThreadId":null,"latestRunId":"run-1","activeRunId":null,"status":"completed","pendingRuntimeRequest":null,"latestVisibleMessage":null,"latestUserMessageAt":null,"hasActionableProposedPlan":false,"itemCount":1,"visibleItemCount":1,"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","archivedAt":null,"settledOverride":null,"settledAt":null,"deletedAt":null})
}
fn config(id: &str, used: i64) -> Value {
    let settings = if id == "remote-a" {
        json!({"environmentIcon":"mac-mini"})
    } else {
        json!({})
    };
    json!({"environment":{"platform":{"machine":"mac-mini"}},"settings":settings,"providers":[{"instanceId":"fixture","driver":"codex","enabled":true,"installed":true,"auth":{"status":"authenticated","email":"fixture@example.invalid"},"usageLimits":{"windows":[{"id":"session","kind":"session","label":"Session","usedPercent":used}]}}],"usageLimitSources":[]})
}
fn usage(id: &str, used: i64, query: &Value) -> Value {
    let hour = query["resolution"] == "hour";
    let at = if hour {
        query["sinceTime"].as_str().unwrap()
    } else {
        "2026-11-06T00:00:00Z"
    };
    let day = if hour {
        &at[..10]
    } else {
        query["sinceDay"].as_str().unwrap()
    };
    let mut bucket = json!({"day":day,"provider":"codex","model":"fixture","sourcePath":"/fixture","totals":{"uncachedInputTokens":used,"cachedInputTokens":0,"cacheCreationTokens":0,"outputTokens":used,"reasoningTokens":0},"costUsd":1,"cacheSavingsUsd":0,"costSource":"modelPriced","records":1,"unpricedRecords":0,"sessions":1});
    if hour {
        bucket["hourStart"] = json!(at);
    }
    let mut summary = json!({"contractVersion":6,"readAt":"2026-11-06T09:08:00Z","timeZone":query["timeZone"],"sinceDay":query["sinceDay"],"untilDay":query["untilDay"],"resolution":query["resolution"],"sinceTime":query["sinceTime"],"untilTime":query["untilTime"],"modelFilter":query["modelFilter"],"buckets":[bucket],"sources":[{"fingerprint":{"hostId":id,"provider":"codex","resolvedHomePath":"/fixture","volumeId":id},"status":"ok","scannedFiles":1,"skippedFiles":0,"malformedRecords":0,"distinctSessions":1,"message":null}],"pricing":{"status":"fresh","source":"fixture","fetchedAt":null,"knownModels":1},"scanDurationMs":1});
    for key in ["modelFilter", "sinceTime", "untilTime"] {
        if query.get(key).is_none() {
            summary.as_object_mut().unwrap().remove(key);
        }
    }
    summary
}
async fn request_line(stream: &TcpStream) -> tokio::io::Result<String> {
    let mut bytes = [0; 16_384];
    loop {
        let count = stream.peek(&mut bytes).await?;
        if let Some(end) = bytes[..count].windows(2).position(|pair| pair == b"\r\n") {
            return Ok(String::from_utf8_lossy(&bytes[..end]).into_owned());
        }
        if count == 0 || count == bytes.len() {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
async fn send_chunk(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    id: u64,
    value: Value,
) {
    socket
        .send(Message::Text(
            json!({"_tag":"Chunk","requestId":id,"values":value})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}
async fn send_success(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    id: u64,
    value: Value,
) {
    socket
        .send(Message::Text(
            json!({"_tag":"Exit","requestId":id,"exit":{"_tag":"Success","value":value}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}
async fn read_http(stream: &mut TcpStream) -> tokio::io::Result<String> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 16_384 {
        bytes.push(stream.read_u8().await?);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
async fn respond(stream: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    let _ = stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_connection::{
        rpc::{ReadEvent, ReadSession},
        VerifiedEndpoint,
    };
    use bar_identity::EnvironmentId;

    #[test]
    fn absent_usage_filter_is_omitted_instead_of_null() {
        let query = json!({"sinceDay":"2026-11-05","untilDay":"2026-11-06","timeZone":"UTC","resolution":"day"});
        let summary = usage("remote-a", 20, &query);
        assert!(summary.get("modelFilter").is_none());
        assert!(summary.get("sinceTime").is_none());
    }

    #[tokio::test]
    async fn fixture_completes_pairing_websocket_and_hourly_usage() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(handle(stream, "remote-a", "Remote A", 20));
            }
        });
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            let endpoint = VerifiedEndpoint::discover(
                &format!("http://{address}/"),
                &EnvironmentId::new("remote-a").unwrap(),
            )
            .await
            .unwrap();
            let grant = endpoint.pair("ABCD2345EFGH").await.unwrap();
            let mut session = ReadSession::connect(&endpoint, &grant).await.unwrap();
            let request = session
                .get_usage_summary(
                    "2026-11-05",
                    "2026-11-06",
                    "UTC",
                    "hour",
                    Some("2026-11-05T09:08:00.000Z"),
                    Some("2026-11-06T09:08:00.000Z"),
                    None,
                )
                .await
                .unwrap();
            match session.next_event().await.unwrap() {
                ReadEvent::Success { request_id, value } => {
                    assert_eq!(request_id, request);
                    let summary: Value = value.decode().unwrap();
                    assert_eq!(summary["sinceDay"], "2026-11-05");
                    assert_eq!(summary["resolution"], "hour");
                    assert_eq!(
                        summary["buckets"][0]["hourStart"],
                        "2026-11-05T09:08:00.000Z"
                    );
                    assert_eq!(summary["buckets"][0]["totals"]["uncachedInputTokens"], 20);
                }
                event => panic!("unexpected fixture event {event:?}"),
            }
            session.close().await.unwrap();
        })
        .await;
        server.abort();
        outcome.expect("fixture handshake and usage must finish within three seconds");
    }
}
