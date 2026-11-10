//! Read-only Effect RPC session. Payloads remain untrusted until decoded by a caller.

use crate::{ConnectionError, Grant, VerifiedEndpoint};
use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{
        protocol::{Message, WebSocketConfig},
        Error as WsError,
    },
    MaybeTlsStream, WebSocketStream,
};

const MAX_FRAME: usize = 16 * 1024 * 1024;
const MAX_REQUESTS: usize = 128;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RequestId(u64);

/// A server-supplied value. Decode it against the consuming domain's schema.
#[derive(Debug)]
pub struct UntrustedReply(Value);
impl UntrustedReply {
    pub fn decode<T: DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.0)
    }
}

#[derive(Debug)]
pub enum ReadEvent {
    Chunk {
        request_id: RequestId,
        values: Vec<UntrustedReply>,
    },
    Success {
        request_id: RequestId,
        value: UntrustedReply,
    },
    PermissionDenied {
        request_id: RequestId,
    },
    Failure {
        request_id: RequestId,
        cause: UntrustedReply,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum RpcError {
    Connection(ConnectionError),
    Expired,
    InvalidInput,
    TooManyRequests,
    UnknownRequest,
    Protocol,
    Transport,
    Closed,
}

#[derive(Clone, Copy)]
enum Kind {
    Stream,
    Unary,
}

pub struct ReadSession {
    socket: Socket,
    expires_at: Instant,
    next_id: u64,
    active: HashMap<RequestId, Kind>,
    pending: VecDeque<Value>,
}

impl ReadSession {
    pub async fn connect(endpoint: &VerifiedEndpoint, grant: &Grant) -> Result<Self, RpcError> {
        if Instant::now() >= grant.expires_at {
            return Err(RpcError::Expired);
        }
        let ticket = endpoint
            .websocket_ticket(grant)
            .await
            .map_err(RpcError::Connection)?;
        if Instant::now() >= grant.expires_at {
            return Err(RpcError::Expired);
        }
        let url = endpoint
            .websocket_url(grant, &ticket)
            .map_err(RpcError::Connection)?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(MAX_FRAME))
            .max_frame_size(Some(MAX_FRAME));
        let (socket, _) = tokio::time::timeout(
            Duration::from_secs(10),
            connect_async_with_config(url.as_str(), Some(config), false),
        )
        .await
        .map_err(|_| RpcError::Transport)?
        .map_err(|_| RpcError::Transport)?;
        Ok(Self {
            socket,
            expires_at: grant.expires_at,
            next_id: 0,
            active: HashMap::new(),
            pending: VecDeque::new(),
        })
    }

    pub async fn subscribe_shell(
        &mut self,
        after_sequence: Option<u64>,
    ) -> Result<RequestId, RpcError> {
        if after_sequence.is_some_and(|n| n > 9_007_199_254_740_991) {
            return Err(RpcError::InvalidInput);
        }
        let payload = after_sequence.map_or_else(|| json!({}), |n| json!({"afterSequence": n}));
        self.request("orchestration.subscribeShell", payload, Kind::Stream)
            .await
    }
    pub async fn get_thread_projection(&mut self, thread_id: &str) -> Result<RequestId, RpcError> {
        if thread_id.trim().is_empty() || thread_id != thread_id.trim() {
            return Err(RpcError::InvalidInput);
        }
        self.request(
            "orchestration.getThreadProjection",
            json!({"threadId":thread_id}),
            Kind::Unary,
        )
        .await
    }
    pub async fn get_config(&mut self) -> Result<RequestId, RpcError> {
        self.request("server.getConfig", json!({}), Kind::Unary)
            .await
    }
    pub async fn subscribe_server_config(&mut self) -> Result<RequestId, RpcError> {
        self.request(
            "subscribeServerConfig",
            json!({"usageLimitSources":true,"workspaceSnapshotsOnDemand":true}),
            Kind::Stream,
        )
        .await
    }
    pub async fn get_usage_summary(
        &mut self,
        since_day: &str,
        until_day: &str,
        time_zone: &str,
        resolution: &str,
        since_time: Option<&str>,
        until_time: Option<&str>,
        model_filter: Option<&Value>,
    ) -> Result<RequestId, RpcError> {
        fn day(s: &str) -> bool {
            let b = s.as_bytes();
            b.len() == 10
                && b.iter().enumerate().all(|(i, c)| {
                    if i == 4 || i == 7 {
                        *c == b'-'
                    } else {
                        c.is_ascii_digit()
                    }
                })
        }
        if !day(since_day)
            || !day(until_day)
            || time_zone.trim().is_empty()
            || time_zone != time_zone.trim()
            || resolution.trim().is_empty()
            || resolution != resolution.trim()
            || since_time.is_some_and(|value| value.trim().is_empty() || value != value.trim())
            || until_time.is_some_and(|value| value.trim().is_empty() || value != value.trim())
        {
            return Err(RpcError::InvalidInput);
        }
        let mut payload = json!({
            "sinceDay": since_day,
            "untilDay": until_day,
            "timeZone": time_zone,
            "resolution": resolution,
        });
        let object = payload.as_object_mut().ok_or(RpcError::Protocol)?;
        if let Some(value) = since_time {
            object.insert("sinceTime".into(), Value::String(value.into()));
        }
        if let Some(value) = until_time {
            object.insert("untilTime".into(), Value::String(value.into()));
        }
        if let Some(value) = model_filter {
            if !value.is_object() {
                return Err(RpcError::InvalidInput);
            }
            object.insert("modelFilter".into(), value.clone());
        }
        self.request("server.getUsageSummary", payload, Kind::Unary)
            .await
    }
    async fn request(
        &mut self,
        tag: &str,
        payload: Value,
        kind: Kind,
    ) -> Result<RequestId, RpcError> {
        if Instant::now() >= self.expires_at {
            return Err(RpcError::Expired);
        }
        if self.active.len() >= MAX_REQUESTS {
            return Err(RpcError::TooManyRequests);
        }
        let id = RequestId(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .filter(|n| *n <= 9_007_199_254_740_991)
            .ok_or(RpcError::Protocol)?;
        self.send(json!({"_tag":"Request","id":id.0,"tag":tag,"payload":payload,"headers":[]}))
            .await?;
        self.active.insert(id, kind);
        Ok(id)
    }
    async fn send(&mut self, value: Value) -> Result<(), RpcError> {
        let text = serde_json::to_string(&value).map_err(|_| RpcError::Protocol)?;
        if text.len() > MAX_FRAME {
            return Err(RpcError::InvalidInput);
        }
        self.socket
            .send(Message::Text(text.into()))
            .await
            .map_err(map_ws)
    }
    pub async fn cancel(&mut self, id: RequestId) -> Result<(), RpcError> {
        if !self.active.contains_key(&id) {
            return Err(RpcError::UnknownRequest);
        }
        self.send(json!({"_tag":"Interrupt","requestId":id.0,"interruptors":[]}))
            .await?;
        self.active.remove(&id);
        Ok(())
    }
    /// Interrupt every request that is still active on this read session.
    pub async fn cancel_all(&mut self) -> Result<(), RpcError> {
        let ids: Vec<_> = self.active.keys().copied().collect();
        let mut first_error = None;
        for id in ids {
            if let Err(error) = self.cancel(id).await {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
    pub async fn ping(&mut self) -> Result<(), RpcError> {
        self.send(json!({"_tag":"Ping"})).await
    }
    pub async fn close(mut self) -> Result<(), RpcError> {
        self.socket.close(None).await.map_err(map_ws)
    }
    /// Reads one event. Drive this continuously while subscriptions are active.
    pub async fn next_event(&mut self) -> Result<ReadEvent, RpcError> {
        self.next_event_until(self.expires_at).await
    }
    /// Reads one event until the caller's bounded operation deadline.
    pub async fn next_event_until(&mut self, deadline: Instant) -> Result<ReadEvent, RpcError> {
        loop {
            if Instant::now() >= self.expires_at {
                return Err(RpcError::Expired);
            }
            if Instant::now() >= deadline {
                return Err(RpcError::Transport);
            }
            if let Some(value) = self.pending.pop_front() {
                if let Some(event) = self.handle(value).await? {
                    return Ok(event);
                }
                continue;
            }
            let deadline = deadline.min(self.expires_at);
            let grant_deadline = deadline == self.expires_at;
            let message = tokio::time::timeout_at(deadline.into(), self.socket.next())
                .await
                .map_err(|_| {
                    if grant_deadline {
                        RpcError::Expired
                    } else {
                        RpcError::Transport
                    }
                })?;
            let message = message.ok_or(RpcError::Closed)?.map_err(map_ws)?;
            match message {
                Message::Text(s) => {
                    let value: Value = serde_json::from_str(&s).map_err(|_| RpcError::Protocol)?;
                    match value {
                        Value::Array(items) => self.pending.extend(items),
                        v => self.pending.push_back(v),
                    }
                }
                Message::Ping(data) => self
                    .socket
                    .send(Message::Pong(data))
                    .await
                    .map_err(map_ws)?,
                Message::Pong(_) => (),
                Message::Close(_) => return Err(RpcError::Closed),
                _ => return Err(RpcError::Protocol),
            }
        }
    }
    async fn handle(&mut self, value: Value) -> Result<Option<ReadEvent>, RpcError> {
        let obj = value.as_object().ok_or(RpcError::Protocol)?;
        let tag = obj
            .get("_tag")
            .and_then(Value::as_str)
            .ok_or(RpcError::Protocol)?;
        match tag {
            "Ping" => {
                self.send(json!({"_tag":"Pong"})).await?;
                Ok(None)
            }
            "Pong" => Ok(None),
            "Chunk" | "Exit" => {
                let id = RequestId(
                    obj.get("requestId")
                        .and_then(Value::as_u64)
                        .ok_or(RpcError::Protocol)?,
                );
                let Some(kind) = self.active.get(&id).copied() else {
                    return Ok(None);
                };
                if tag == "Chunk" {
                    if !matches!(kind, Kind::Stream) {
                        return Err(RpcError::Protocol);
                    }
                    let values = obj
                        .get("values")
                        .and_then(Value::as_array)
                        .filter(|v| !v.is_empty())
                        .ok_or(RpcError::Protocol)?
                        .iter()
                        .cloned()
                        .map(UntrustedReply)
                        .collect();
                    self.send(json!({"_tag":"Ack","requestId":id.0})).await?;
                    Ok(Some(ReadEvent::Chunk {
                        request_id: id,
                        values,
                    }))
                } else {
                    let exit = obj
                        .get("exit")
                        .and_then(Value::as_object)
                        .ok_or(RpcError::Protocol)?;
                    let event = match exit.get("_tag").and_then(Value::as_str) {
                        Some("Success") => ReadEvent::Success {
                            request_id: id,
                            value: UntrustedReply(
                                exit.get("value").cloned().unwrap_or(Value::Null),
                            ),
                        },
                        Some("Failure") => {
                            let cause = exit
                                .get("cause")
                                .and_then(Value::as_array)
                                .filter(|c| !c.is_empty())
                                .ok_or(RpcError::Protocol)?;
                            if cause.iter().any(|c| {
                                c.get("_tag").and_then(Value::as_str) == Some("Fail")
                                    && c.pointer("/error/_tag").and_then(Value::as_str)
                                        == Some("EnvironmentAuthorizationError")
                            }) {
                                ReadEvent::PermissionDenied { request_id: id }
                            } else {
                                ReadEvent::Failure {
                                    request_id: id,
                                    cause: UntrustedReply(Value::Array(cause.clone())),
                                }
                            }
                        }
                        _ => return Err(RpcError::Protocol),
                    };
                    self.active.remove(&id);
                    Ok(Some(event))
                }
            }
            _ => Err(RpcError::Protocol),
        }
    }
}
fn map_ws(error: WsError) -> RpcError {
    match error {
        WsError::ConnectionClosed | WsError::AlreadyClosed => RpcError::Closed,
        WsError::Capacity(_) | WsError::Protocol(_) | WsError::Utf8 => RpcError::Protocol,
        _ => RpcError::Transport,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bar_identity::EnvironmentId;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::handshake::server::{Request, Response},
    };
    use zeroize::Zeroizing;

    async fn http(stream: &mut TcpStream, body: &str, path: &str) {
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(stream.read_u8().await.unwrap());
        }
        let input = String::from_utf8(head).unwrap();
        assert!(input.starts_with(path), "{input}");
        if path.starts_with("POST") {
            assert!(input
                .to_lowercase()
                .contains("authorization: bearer synthetic-token"));
        }
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        stream.write_all(response.as_bytes()).await.unwrap();
    }
    fn grant() -> Grant {
        Grant {
            environment: EnvironmentId::new("env-a").unwrap(),
            token: Zeroizing::new("synthetic-token".into()),
            origin: "http://127.0.0.1/".into(),
            expires_wall: std::time::SystemTime::now() + Duration::from_secs(3600),
            expires_at: Instant::now() + Duration::from_secs(60),
            orchestration_read: true,
            diagnostics_read: false,
            dpop: None,
        }
    }
    async fn setup() -> (ReadSession, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let descriptor = r#"{"environmentId":"env-a","label":"Test","platform":{"os":"linux","arch":"x64"},"serverVersion":"1","capabilities":{}}"#;
            for _ in 0..2 {
                let (mut s, _) = listener.accept().await.unwrap();
                http(&mut s, descriptor, "GET /.well-known/t3/environment ").await;
            }
            let (mut s, _) = listener.accept().await.unwrap();
            http(
                &mut s,
                r#"{"ticket":"synthetic-ticket","expiresAt":"2099-01-01T00:00:00Z"}"#,
                "POST /api/auth/websocket-ticket/rpc ",
            )
            .await;
            let (s, _) = listener.accept().await.unwrap();
            let mut ws = accept_hdr_async(s, |req: &Request, response: Response| {
                assert_eq!(
                    req.uri().path_and_query().unwrap().as_str(),
                    "/ws?wsTicket=synthetic-ticket"
                );
                Ok(response)
            })
            .await
            .unwrap();
            let mut requests = Vec::new();
            for (tag, payload) in [
                ("orchestration.subscribeShell", json!({"afterSequence":7})),
                (
                    "orchestration.getThreadProjection",
                    json!({"threadId":"thread-1"}),
                ),
                ("server.getConfig", json!({})),
                (
                    "subscribeServerConfig",
                    json!({"usageLimitSources":true,"workspaceSnapshotsOnDemand":true}),
                ),
                (
                    "server.getUsageSummary",
                    json!({"sinceDay":"2026-01-01","untilDay":"2026-01-31","timeZone":"UTC","resolution":"day","sinceTime":"2026-01-01T00:00:00Z","untilTime":"2026-01-31T00:00:00Z","modelFilter":{"include":[{"provider":"codex","model":"gpt-5.4"}]}}),
                ),
            ] {
                let msg = ws.next().await.unwrap().unwrap().into_text().unwrap();
                let req: Value = serde_json::from_str(&msg).unwrap();
                assert_eq!(req["_tag"], "Request");
                assert_eq!(req["tag"], tag);
                assert_eq!(req["payload"], payload);
                assert_eq!(req["headers"], json!([]));
                requests.push(req["id"].clone());
            }
            ws.send(Message::Text(json!([
                {"_tag":"Chunk","requestId":99999,"values":["wrong"]},
                {"_tag":"Exit","requestId":requests[4],"exit":{"_tag":"Failure","cause":[{"_tag":"Fail","error":{"_tag":"EnvironmentAuthorizationError","message":"denied","requiredScope":"diagnostics:read"}}]}},
                {"_tag":"Chunk","requestId":requests[0],"values":[{"shell":1},{"shell":2}]},
                {"_tag":"Exit","requestId":requests[1],"exit":{"_tag":"Failure","cause":[{"_tag":"Fail","error":{"_tag":"ReadError"}}]}},
                {"_tag":"Exit","requestId":requests[2],"exit":{"_tag":"Success","value":{"config":1}}},
                {"_tag":"Chunk","requestId":requests[3],"values":[{"config":2}]}
            ]).to_string().into())).await.unwrap();
            let ack: Value =
                serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(ack, json!({"_tag":"Ack","requestId":requests[0]}));
            let ack: Value =
                serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(ack, json!({"_tag":"Ack","requestId":requests[3]}));
            let interrupt: Value =
                serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(
                interrupt,
                json!({"_tag":"Interrupt","requestId":requests[3],"interruptors":[]})
            );
            let ping: Value =
                serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(ping, json!({"_tag":"Ping"}));
            ws.send(Message::Text(json!({"_tag":"Ping"}).to_string().into()))
                .await
                .unwrap();
            let pong: Value =
                serde_json::from_str(&ws.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(pong, json!({"_tag":"Pong"}));
            ws.send(Message::Text("not-json".into())).await.unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
        });
        let endpoint = VerifiedEndpoint::discover(
            &format!("http://{addr}/"),
            &EnvironmentId::new("env-a").unwrap(),
        )
        .await
        .unwrap();
        let mut grant = grant();
        grant.origin = endpoint.base.as_str().to_owned();
        let session = ReadSession::connect(&endpoint, &grant).await.unwrap();
        (session, server)
    }
    #[tokio::test]
    async fn reads_and_routes_effect_messages_over_verified_socket() {
        let (mut session, server) = setup().await;
        let shell = session.subscribe_shell(Some(7)).await.unwrap();
        let thread = session.get_thread_projection("thread-1").await.unwrap();
        let config = session.get_config().await.unwrap();
        let config_stream = session.subscribe_server_config().await.unwrap();
        let usage = session
            .get_usage_summary(
                "2026-01-01",
                "2026-01-31",
                "UTC",
                "day",
                Some("2026-01-01T00:00:00Z"),
                Some("2026-01-31T00:00:00Z"),
                Some(&json!({"include":[{"provider":"codex","model":"gpt-5.4"}]})),
            )
            .await
            .unwrap();
        assert!(
            matches!(session.next_event().await.unwrap(), ReadEvent::PermissionDenied { request_id } if request_id == usage)
        );
        match session.next_event().await.unwrap() {
            ReadEvent::Chunk { request_id, values } => {
                assert_eq!(request_id, shell);
                assert_eq!(values.len(), 2);
                assert_eq!(
                    values
                        .into_iter()
                        .next()
                        .unwrap()
                        .decode::<Value>()
                        .unwrap(),
                    json!({"shell":1})
                );
            }
            event => panic!("{event:?}"),
        }
        assert!(
            matches!(session.next_event().await.unwrap(), ReadEvent::Failure { request_id, .. } if request_id == thread)
        );
        assert!(
            matches!(session.next_event().await.unwrap(), ReadEvent::Success { request_id, .. } if request_id == config)
        );
        assert!(
            matches!(session.next_event().await.unwrap(), ReadEvent::Chunk { request_id, .. } if request_id == config_stream)
        );
        session.cancel(config_stream).await.unwrap();
        session.ping().await.unwrap();
        assert!(matches!(
            session.next_event().await,
            Err(RpcError::Protocol)
        ));
        session.close().await.unwrap();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn expired_grant_rejected_before_network() {
        let id = EnvironmentId::new("env-a").unwrap();
        let endpoint = VerifiedEndpoint {
            client: reqwest::Client::new(),
            base: url::Url::parse("http://127.0.0.1:1/").unwrap(),
            ws_base: url::Url::parse("ws://127.0.0.1:1/").unwrap(),
            descriptor: crate::EnvironmentDescriptor {
                id,
                label: "Test".into(),
                machine: None,
                server_version: "1".into(),
            },
            credential_identity: None,
        };
        let mut grant = grant();
        grant.expires_at = Instant::now();
        assert!(matches!(
            ReadSession::connect(&endpoint, &grant).await,
            Err(RpcError::Expired)
        ));
    }

    #[tokio::test]
    async fn websocket_ping_and_peer_close_are_handled() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            ws.send(Message::Ping(vec![1, 2].into())).await.unwrap();
            assert!(
                matches!(ws.next().await.unwrap().unwrap(), Message::Pong(payload) if payload.as_ref() == [1, 2])
            );
            ws.close(None).await.unwrap();
        });
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/"))
            .await
            .unwrap();
        let mut session = ReadSession {
            socket,
            expires_at: Instant::now() + Duration::from_secs(10),
            next_id: 0,
            active: HashMap::new(),
            pending: VecDeque::new(),
        };
        assert!(matches!(session.next_event().await, Err(RpcError::Closed)));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn silent_server_obeys_operation_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(message)) = ws.next().await {
                if matches!(message, Message::Close(_)) {
                    break;
                }
            }
        });
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/"))
            .await
            .unwrap();
        let mut session = ReadSession {
            socket,
            expires_at: Instant::now() + Duration::from_secs(60),
            next_id: 0,
            active: HashMap::new(),
            pending: VecDeque::new(),
        };
        session.get_config().await.unwrap();
        assert_eq!(
            session
                .next_event_until(Instant::now() + Duration::from_millis(50))
                .await
                .unwrap_err(),
            RpcError::Transport,
        );
        session.cancel_all().await.unwrap();
        session.close().await.unwrap();
        server.await.unwrap();
    }
}
