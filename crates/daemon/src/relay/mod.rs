// Relay connection manager: WebSocket client with reconnection.
//
// Manages the daemon's connection to the relay server for multi-user
// collaboration. Handles authentication, document subscription,
// outbox draining, and incoming update application.
//
// Transport is abstracted via `RelayTransport` for testability.
// The actual WS transport implementation lives in a separate module.

pub mod mdns;

use std::collections::HashSet;
use std::net::IpAddr;
use std::time::Duration;

use anyhow::{anyhow, Result};
use tracing::info;
use url::Url;
use uuid::Uuid;

use scriptum_common::protocol::ws::{WsMessage, CURRENT_PROTOCOL_VERSION as WS_PROTOCOL_VERSION};

// ── Configuration ───────────────────────────────────────────────────

/// Connection parameters for the relay.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Relay server base URL (e.g. "https://relay.example.com").
    pub relay_url: String,
    /// Workspace to connect to.
    pub workspace_id: Uuid,
    /// Bearer token for REST API auth.
    pub auth_token: String,
    /// Stable client identifier for this daemon instance.
    pub client_id: Uuid,
    /// Device identifier.
    pub device_id: Uuid,
}

/// Reconnection parameters.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub max_attempts: u32,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(250),
            max_delay: Duration::from_secs(30),
            max_attempts: u32::MAX, // retry indefinitely
        }
    }
}

// ── Transport trait ─────────────────────────────────────────────────

/// Session info returned by the REST session-creation endpoint.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub session_token: String,
    pub ws_url: String,
    pub resume_token: String,
    pub heartbeat_interval_ms: u64,
}

/// Abstraction over the network transport for testability.
///
/// In production this would use reqwest + tokio-tungstenite.
/// In tests it can be a mock that records messages.
pub trait RelayTransport {
    /// Create a sync session via the REST API.
    fn create_session(
        &mut self,
        config: &RelayConfig,
        resume_token: Option<&str>,
    ) -> Result<SessionInfo>;

    /// Open a WebSocket connection to the given URL.
    fn connect_ws(&mut self, ws_url: &str) -> Result<()>;

    /// Send a message over the WebSocket.
    fn send(&mut self, msg: &WsMessage) -> Result<()>;

    /// Receive the next message (blocking). Returns None on clean close.
    fn recv(&mut self) -> Result<Option<WsMessage>>;

    /// Close the WebSocket.
    fn close(&mut self);
}

// ── Connection state ────────────────────────────────────────────────

/// Current state of the relay connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Authenticating,
    Connected,
}

// ── Incoming event ──────────────────────────────────────────────────

/// Events emitted by the connection manager for the daemon to handle.
#[derive(Debug, Clone, PartialEq)]
pub enum RelayEvent {
    /// Successfully connected and authenticated.
    Connected,
    /// Received a full document snapshot.
    Snapshot { doc_id: Uuid, snapshot_seq: i64, payload_b64: String },
    /// Received a remote YJS update.
    RemoteUpdate {
        doc_id: Uuid,
        client_id: Uuid,
        client_update_id: Uuid,
        base_server_seq: i64,
        payload_b64: String,
    },
    /// An outbox update was acknowledged by the relay.
    UpdateAcked { doc_id: Uuid, client_update_id: Uuid, server_seq: i64 },
    /// Connection lost, will retry.
    Disconnected { reason: String },
    /// A protocol error from the relay.
    Error { code: String, message: String, retryable: bool },
}

// ── Connection manager ──────────────────────────────────────────────

/// Manages the relay connection lifecycle.
pub struct RelayConnectionManager<T: RelayTransport> {
    config: RelayConfig,
    reconnect_policy: ReconnectPolicy,
    transport: T,
    state: ConnectionState,
    session_token: Option<String>,
    resume_token: Option<String>,
    subscribed_docs: HashSet<Uuid>,
    consecutive_failures: u32,
}

impl<T: RelayTransport> RelayConnectionManager<T> {
    pub fn new(config: RelayConfig, transport: T) -> Self {
        Self {
            config,
            reconnect_policy: ReconnectPolicy::default(),
            transport,
            state: ConnectionState::Disconnected,
            session_token: None,
            resume_token: None,
            subscribed_docs: HashSet::new(),
            consecutive_failures: 0,
        }
    }

    pub fn with_reconnect_policy(mut self, policy: ReconnectPolicy) -> Self {
        self.reconnect_policy = policy;
        self
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    pub fn subscribed_docs(&self) -> &HashSet<Uuid> {
        &self.subscribed_docs
    }

    /// Discover LAN peers advertising Scriptum sync via mDNS.
    ///
    /// This enables zero-config peer discovery for direct TCP optimization
    /// while keeping relay as the always-on durable transport.
    pub fn discover_lan_peers(&self, timeout: Duration) -> Result<Vec<mdns::LanPeerEndpoint>> {
        mdns::discover_lan_peers(self.config.workspace_id, timeout).map_err(|error| anyhow!(error))
    }

    /// Attempt to connect (or reconnect) to the relay.
    ///
    /// Returns `Connected` event on success, `Disconnected` on failure.
    pub fn connect(&mut self) -> Result<RelayEvent> {
        validate_relay_url(&self.config.relay_url)?;
        self.state = ConnectionState::Connecting;

        // Step 1: Create session via REST API.
        let session =
            match self.transport.create_session(&self.config, self.resume_token.as_deref()) {
                Ok(session) => session,
                Err(e) => {
                    self.state = ConnectionState::Disconnected;
                    self.consecutive_failures += 1;
                    return Ok(RelayEvent::Disconnected {
                        reason: format!("session creation failed: {e}"),
                    });
                }
            };

        validate_ws_url(&session.ws_url)?;

        // Step 2: Open WebSocket.
        if let Err(e) = self.transport.connect_ws(&session.ws_url) {
            self.state = ConnectionState::Disconnected;
            self.consecutive_failures += 1;
            return Ok(RelayEvent::Disconnected {
                reason: format!("WebSocket connection failed: {e}"),
            });
        }

        // Step 3: Send Hello frame.
        self.state = ConnectionState::Authenticating;
        let hello = WsMessage::Hello {
            protocol_version: WS_PROTOCOL_VERSION.to_string(),
            session_token: session.session_token.clone(),
            resume_token: self.resume_token.clone(),
        };
        if let Err(e) = self.transport.send(&hello) {
            self.transport.close();
            self.state = ConnectionState::Disconnected;
            self.consecutive_failures += 1;
            return Ok(RelayEvent::Disconnected { reason: format!("failed to send hello: {e}") });
        }

        // Step 4: Wait for HelloAck.
        let hello_resume_token = match self.transport.recv() {
            Ok(Some(WsMessage::HelloAck { resume_accepted, resume_token, .. })) => {
                if !resume_accepted {
                    // Session wasn't resumed — need to resubscribe.
                    self.subscribed_docs.clear();
                }
                info!(
                    session_id = %session.session_id,
                    resume_accepted,
                    "relay connection established"
                );
                resume_token
            }
            Ok(Some(WsMessage::Error { code, message, .. })) => {
                self.transport.close();
                self.state = ConnectionState::Disconnected;
                self.consecutive_failures += 1;
                return Ok(RelayEvent::Disconnected {
                    reason: format!("hello rejected: {code}: {message}"),
                });
            }
            Ok(Some(_)) => {
                self.transport.close();
                self.state = ConnectionState::Disconnected;
                self.consecutive_failures += 1;
                return Ok(RelayEvent::Disconnected {
                    reason: "unexpected message in response to hello".to_string(),
                });
            }
            Ok(None) => {
                self.state = ConnectionState::Disconnected;
                self.consecutive_failures += 1;
                return Ok(RelayEvent::Disconnected {
                    reason: "connection closed during handshake".to_string(),
                });
            }
            Err(e) => {
                self.transport.close();
                self.state = ConnectionState::Disconnected;
                self.consecutive_failures += 1;
                return Ok(RelayEvent::Disconnected {
                    reason: format!("error during handshake: {e}"),
                });
            }
        };

        self.session_token = Some(session.session_token);
        self.resume_token = Some(hello_resume_token);
        self.state = ConnectionState::Connected;
        self.consecutive_failures = 0;

        Ok(RelayEvent::Connected)
    }

    /// Subscribe to a document's updates.
    pub fn subscribe(&mut self, doc_id: Uuid, last_server_seq: Option<i64>) -> Result<()> {
        if self.state != ConnectionState::Connected {
            return Err(anyhow!("cannot subscribe: not connected"));
        }

        let msg = WsMessage::Subscribe { doc_id, last_server_seq };
        self.transport.send(&msg)?;
        self.subscribed_docs.insert(doc_id);
        Ok(())
    }

    /// Send a YJS update to the relay.
    pub fn send_update(
        &mut self,
        doc_id: Uuid,
        client_update_id: Uuid,
        base_server_seq: i64,
        payload_b64: String,
    ) -> Result<()> {
        if self.state != ConnectionState::Connected {
            return Err(anyhow!("cannot send update: not connected"));
        }

        let msg = WsMessage::YjsUpdate {
            doc_id,
            client_id: self.config.client_id,
            client_update_id,
            base_server_seq,
            payload_b64,
        };
        self.transport.send(&msg)
    }

    /// Process the next incoming message. Returns None on clean close.
    pub fn recv_event(&mut self) -> Result<Option<RelayEvent>> {
        if self.state != ConnectionState::Connected {
            return Err(anyhow!("cannot receive: not connected"));
        }

        match self.transport.recv()? {
            Some(WsMessage::Snapshot { doc_id, snapshot_seq, payload_b64 }) => {
                Ok(Some(RelayEvent::Snapshot { doc_id, snapshot_seq, payload_b64 }))
            }

            Some(WsMessage::YjsUpdate {
                doc_id,
                client_id,
                client_update_id,
                base_server_seq,
                payload_b64,
            }) => Ok(Some(RelayEvent::RemoteUpdate {
                doc_id,
                client_id,
                client_update_id,
                base_server_seq,
                payload_b64,
            })),

            Some(WsMessage::Ack { doc_id, client_update_id, server_seq, .. }) => {
                Ok(Some(RelayEvent::UpdateAcked { doc_id, client_update_id, server_seq }))
            }

            Some(WsMessage::Error { code, message, retryable, .. }) => {
                Ok(Some(RelayEvent::Error { code, message, retryable }))
            }

            Some(_) => {
                // Ignore unknown/unexpected messages.
                Ok(None)
            }

            None => {
                // Connection closed.
                self.state = ConnectionState::Disconnected;
                Ok(Some(RelayEvent::Disconnected {
                    reason: "connection closed by server".to_string(),
                }))
            }
        }
    }

    /// Disconnect from the relay.
    pub fn disconnect(&mut self) {
        self.transport.close();
        self.state = ConnectionState::Disconnected;
    }

    /// Compute the backoff delay for the next reconnection attempt.
    pub fn reconnect_delay(&self) -> Duration {
        let exp = self.consecutive_failures.min(7);
        let delay =
            DurationSaturatingMul::saturating_mul(self.reconnect_policy.base_delay, 1u64 << exp);
        delay.min(self.reconnect_policy.max_delay)
    }

    /// Whether we should attempt reconnection (under max_attempts).
    pub fn should_reconnect(&self) -> bool {
        self.consecutive_failures < self.reconnect_policy.max_attempts
    }
}

fn validate_relay_url(value: &str) -> Result<()> {
    let parsed =
        Url::parse(value).map_err(|error| anyhow!("invalid relay_url `{value}`: {error}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if is_loopback_host(parsed.host_str()) => Ok(()),
        _ => Err(anyhow!("relay_url must use https (http is allowed only for localhost testing)")),
    }
}

fn validate_ws_url(value: &str) -> Result<()> {
    let parsed = Url::parse(value).map_err(|error| anyhow!("invalid ws_url `{value}`: {error}"))?;
    match parsed.scheme() {
        "wss" => Ok(()),
        "ws" if is_loopback_host(parsed.host_str()) => Ok(()),
        _ => Err(anyhow!("ws_url must use wss (ws is allowed only for localhost testing)")),
    }
}

fn is_loopback_host(host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>().is_ok_and(|addr| addr.is_loopback())
}

// ── Backoff helper (for Duration::saturating_mul with u64) ──────────

trait DurationSaturatingMul {
    fn saturating_mul(self, rhs: u64) -> Self;
}

impl DurationSaturatingMul for Duration {
    fn saturating_mul(self, rhs: u64) -> Self {
        let nanos = self.as_nanos().saturating_mul(rhs as u128);
        if nanos > u64::MAX as u128 {
            Duration::from_secs(u64::MAX)
        } else {
            Duration::from_nanos(nanos as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    // ── Mock transport ──────────────────────────────────────────────

    #[derive(Debug, Default)]
    struct MockTransport {
        /// Responses to be returned by recv() in order.
        recv_queue: VecDeque<Option<WsMessage>>,
        /// Messages sent via send().
        sent: Vec<WsMessage>,
        /// Whether connect_ws was called.
        ws_connected: bool,
        /// Whether close was called.
        closed: bool,
        /// If set, create_session returns this error.
        session_error: Option<String>,
        /// If set, connect_ws returns this error.
        ws_error: Option<String>,
        /// Session info to return.
        session_info: Option<SessionInfo>,
    }

    impl MockTransport {
        fn with_session(session: SessionInfo) -> Self {
            Self { session_info: Some(session), ..Default::default() }
        }

        fn queue_recv(&mut self, msg: WsMessage) {
            self.recv_queue.push_back(Some(msg));
        }

        fn queue_close(&mut self) {
            self.recv_queue.push_back(None);
        }
    }

    impl RelayTransport for MockTransport {
        fn create_session(
            &mut self,
            _config: &RelayConfig,
            _resume_token: Option<&str>,
        ) -> Result<SessionInfo> {
            if let Some(err) = &self.session_error {
                return Err(anyhow!("{}", err));
            }
            self.session_info.clone().ok_or_else(|| anyhow!("no session configured"))
        }

        fn connect_ws(&mut self, _ws_url: &str) -> Result<()> {
            if let Some(err) = &self.ws_error {
                return Err(anyhow!("{}", err));
            }
            self.ws_connected = true;
            Ok(())
        }

        fn send(&mut self, msg: &WsMessage) -> Result<()> {
            self.sent.push(msg.clone());
            Ok(())
        }

        fn recv(&mut self) -> Result<Option<WsMessage>> {
            Ok(self.recv_queue.pop_front().flatten())
        }

        fn close(&mut self) {
            self.closed = true;
            self.ws_connected = false;
        }
    }

    fn test_config() -> RelayConfig {
        RelayConfig {
            relay_url: "https://relay.test".to_string(),
            workspace_id: Uuid::new_v4(),
            auth_token: "test-token".to_string(),
            client_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        }
    }

    fn test_session() -> SessionInfo {
        SessionInfo {
            session_id: Uuid::new_v4(),
            session_token: "sess-tok-123".to_string(),
            ws_url: "wss://relay.test/v1/ws/test".to_string(),
            resume_token: "resume-tok-456".to_string(),
            heartbeat_interval_ms: 15_000,
        }
    }

    fn hello_ack(resume_accepted: bool) -> WsMessage {
        hello_ack_with_token(resume_accepted, "resume-tok-next")
    }

    fn hello_ack_with_token(resume_accepted: bool, resume_token: &str) -> WsMessage {
        WsMessage::HelloAck {
            server_time: "2026-01-01T00:00:00Z".to_string(),
            resume_accepted,
            resume_token: resume_token.to_string(),
            resume_expires_at: "2026-01-01T00:10:00Z".to_string(),
        }
    }

    // ── Connection lifecycle ────────────────────────────────────────

    #[test]
    fn connect_happy_path() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        assert_eq!(mgr.state(), ConnectionState::Disconnected);

        let event = mgr.connect().expect("connect should succeed");
        assert_eq!(event, RelayEvent::Connected);
        assert_eq!(mgr.state(), ConnectionState::Connected);
    }

    #[test]
    fn connect_rejects_non_tls_relay_url() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));

        let mut config = test_config();
        config.relay_url = "http://relay.test".to_string();
        let mut mgr = RelayConnectionManager::new(config, transport);

        let error = mgr.connect().expect_err("connect should reject insecure relay url");
        assert!(error.to_string().contains("relay_url must use https"));
    }

    #[test]
    fn connect_rejects_non_tls_ws_url() {
        let mut session = test_session();
        session.ws_url = "ws://relay.test/v1/ws/test".to_string();
        let mut transport = MockTransport::with_session(session);
        transport.queue_recv(hello_ack(false));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        let error = mgr.connect().expect_err("connect should reject insecure ws url");
        assert!(error.to_string().contains("ws_url must use wss"));
    }

    #[test]
    fn connect_sends_hello_with_session_token() {
        let session = test_session();
        let expected_token = session.session_token.clone();

        let mut transport = MockTransport::with_session(session);
        transport.queue_recv(hello_ack(false));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");

        let hello = &mgr.transport.sent[0];
        match hello {
            WsMessage::Hello { session_token, .. } => {
                assert_eq!(session_token, &expected_token);
            }
            _ => panic!("first message should be Hello"),
        }
    }

    #[test]
    fn connect_updates_resume_token_from_hello_ack() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack_with_token(false, "resume-from-hello-ack"));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect should succeed");

        assert_eq!(
            mgr.resume_token.as_deref(),
            Some("resume-from-hello-ack"),
            "client should store rotated resume token from hello_ack",
        );
    }

    #[test]
    fn connect_fails_on_session_creation_error() {
        let mut transport = MockTransport::default();
        transport.session_error = Some("network error".to_string());

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        let event = mgr.connect().expect("should return event");

        match event {
            RelayEvent::Disconnected { reason } => {
                assert!(reason.contains("session creation failed"));
            }
            _ => panic!("expected Disconnected event"),
        }
        assert_eq!(mgr.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn connect_fails_on_ws_error() {
        let mut transport = MockTransport::with_session(test_session());
        transport.ws_error = Some("refused".to_string());

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        let event = mgr.connect().expect("should return event");

        match event {
            RelayEvent::Disconnected { reason } => {
                assert!(reason.contains("WebSocket connection failed"));
            }
            _ => panic!("expected Disconnected event"),
        }
    }

    #[test]
    fn connect_fails_on_hello_error_response() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(WsMessage::Error {
            code: "SYNC_TOKEN_INVALID".to_string(),
            message: "bad token".to_string(),
            retryable: false,
            doc_id: None,
        });

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        let event = mgr.connect().expect("should return event");

        match event {
            RelayEvent::Disconnected { reason } => {
                assert!(reason.contains("hello rejected"));
            }
            _ => panic!("expected Disconnected event"),
        }
    }

    #[test]
    fn resume_accepted_preserves_subscriptions() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(true));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        // Pre-populate subscriptions as if we were previously connected.
        mgr.subscribed_docs.insert(Uuid::new_v4());
        mgr.subscribed_docs.insert(Uuid::new_v4());

        mgr.connect().expect("connect");
        // resume_accepted = true → subscriptions preserved.
        assert_eq!(mgr.subscribed_docs.len(), 2);
    }

    #[test]
    fn resume_not_accepted_clears_subscriptions() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.subscribed_docs.insert(Uuid::new_v4());

        mgr.connect().expect("connect");
        assert!(mgr.subscribed_docs.is_empty());
    }

    // ── Subscribe ───────────────────────────────────────────────────

    #[test]
    fn subscribe_sends_message_and_tracks_doc() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");

        let doc_id = Uuid::new_v4();
        mgr.subscribe(doc_id, Some(0)).expect("subscribe");

        assert!(mgr.subscribed_docs.contains(&doc_id));
        // Hello + Subscribe = 2 messages sent.
        assert_eq!(mgr.transport.sent.len(), 2);
    }

    #[test]
    fn subscribe_fails_when_not_connected() {
        let transport = MockTransport::default();
        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        assert!(mgr.subscribe(Uuid::new_v4(), None).is_err());
    }

    // ── Send update ─────────────────────────────────────────────────

    #[test]
    fn send_update_sends_yjs_message() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));

        let config = test_config();
        let client_id = config.client_id;
        let mut mgr = RelayConnectionManager::new(config, transport);
        mgr.connect().expect("connect");

        let doc_id = Uuid::new_v4();
        let upd_id = Uuid::new_v4();
        mgr.send_update(doc_id, upd_id, 0, "AAAA".to_string()).expect("send_update");

        let msg = &mgr.transport.sent[1]; // [0] is Hello
        match msg {
            WsMessage::YjsUpdate {
                doc_id: d,
                client_id: c,
                client_update_id: u,
                base_server_seq: s,
                payload_b64: p,
            } => {
                assert_eq!(*d, doc_id);
                assert_eq!(*c, client_id);
                assert_eq!(*u, upd_id);
                assert_eq!(*s, 0);
                assert_eq!(p, "AAAA");
            }
            _ => panic!("expected YjsUpdate"),
        }
    }

    #[test]
    fn send_update_fails_when_disconnected() {
        let transport = MockTransport::default();
        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        assert!(mgr.send_update(Uuid::new_v4(), Uuid::new_v4(), 0, "x".into()).is_err());
    }

    // ── Receive events ──────────────────────────────────────────────

    #[test]
    fn recv_snapshot_event() {
        let doc_id = Uuid::new_v4();
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));
        transport.queue_recv(WsMessage::Snapshot {
            doc_id,
            snapshot_seq: 5,
            payload_b64: "snap".to_string(),
        });

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");

        let event = mgr.recv_event().expect("recv").expect("event");
        assert_eq!(
            event,
            RelayEvent::Snapshot { doc_id, snapshot_seq: 5, payload_b64: "snap".to_string() }
        );
    }

    #[test]
    fn recv_remote_update_event() {
        let doc_id = Uuid::new_v4();
        let client_id = Uuid::new_v4();
        let upd_id = Uuid::new_v4();

        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));
        transport.queue_recv(WsMessage::YjsUpdate {
            doc_id,
            client_id,
            client_update_id: upd_id,
            base_server_seq: 3,
            payload_b64: "data".to_string(),
        });

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");

        let event = mgr.recv_event().expect("recv").expect("event");
        assert_eq!(
            event,
            RelayEvent::RemoteUpdate {
                doc_id,
                client_id,
                client_update_id: upd_id,
                base_server_seq: 3,
                payload_b64: "data".to_string(),
            }
        );
    }

    #[test]
    fn recv_ack_event() {
        let doc_id = Uuid::new_v4();
        let upd_id = Uuid::new_v4();

        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));
        transport.queue_recv(WsMessage::Ack {
            doc_id,
            client_update_id: upd_id,
            server_seq: 7,
            applied: true,
        });

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");

        let event = mgr.recv_event().expect("recv").expect("event");
        assert_eq!(
            event,
            RelayEvent::UpdateAcked { doc_id, client_update_id: upd_id, server_seq: 7 }
        );
    }

    #[test]
    fn recv_error_event() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));
        transport.queue_recv(WsMessage::Error {
            code: "SYNC_BASE_SERVER_SEQ_MISMATCH".to_string(),
            message: "stale".to_string(),
            retryable: true,
            doc_id: None,
        });

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");

        let event = mgr.recv_event().expect("recv").expect("event");
        assert_eq!(
            event,
            RelayEvent::Error {
                code: "SYNC_BASE_SERVER_SEQ_MISMATCH".to_string(),
                message: "stale".to_string(),
                retryable: true,
            }
        );
    }

    #[test]
    fn recv_connection_close_sets_disconnected() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));
        transport.queue_close();

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");

        let event = mgr.recv_event().expect("recv").expect("event");
        match event {
            RelayEvent::Disconnected { .. } => {}
            _ => panic!("expected Disconnected"),
        }
        assert_eq!(mgr.state(), ConnectionState::Disconnected);
    }

    // ── Reconnection backoff ────────────────────────────────────────

    #[test]
    fn reconnect_delay_starts_at_base() {
        let transport = MockTransport::default();
        let mgr = RelayConnectionManager::new(test_config(), transport);
        assert_eq!(mgr.reconnect_delay(), Duration::from_millis(250));
    }

    #[test]
    fn reconnect_delay_increases_with_failures() {
        let mut transport = MockTransport::default();
        transport.session_error = Some("fail".to_string());

        let mut mgr = RelayConnectionManager::new(test_config(), transport);

        // Each failed connect increments consecutive_failures.
        mgr.connect().unwrap();
        assert_eq!(mgr.reconnect_delay(), Duration::from_millis(500));

        mgr.connect().unwrap();
        assert_eq!(mgr.reconnect_delay(), Duration::from_millis(1000));

        mgr.connect().unwrap();
        assert_eq!(mgr.reconnect_delay(), Duration::from_millis(2000));
    }

    #[test]
    fn reconnect_delay_caps_at_max() {
        let mut transport = MockTransport::default();
        transport.session_error = Some("fail".to_string());

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        // Force many failures.
        for _ in 0..20 {
            mgr.connect().unwrap();
        }
        assert_eq!(mgr.reconnect_delay(), Duration::from_secs(30));
    }

    #[test]
    fn successful_connect_resets_failure_count() {
        let mut transport = MockTransport::default();
        transport.session_error = Some("fail".to_string());

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().unwrap();
        mgr.connect().unwrap();
        assert!(mgr.consecutive_failures >= 2);

        // Now make it succeed.
        mgr.transport.session_error = None;
        mgr.transport.session_info = Some(test_session());
        mgr.transport.queue_recv(hello_ack(false));
        mgr.connect().unwrap();

        assert_eq!(mgr.consecutive_failures, 0);
        assert_eq!(mgr.reconnect_delay(), Duration::from_millis(250));
    }

    #[test]
    fn should_reconnect_respects_max_attempts() {
        let policy = ReconnectPolicy { max_attempts: 3, ..Default::default() };
        let mut transport = MockTransport::default();
        transport.session_error = Some("fail".to_string());

        let mut mgr =
            RelayConnectionManager::new(test_config(), transport).with_reconnect_policy(policy);

        assert!(mgr.should_reconnect());
        mgr.connect().unwrap();
        assert!(mgr.should_reconnect());
        mgr.connect().unwrap();
        assert!(mgr.should_reconnect());
        mgr.connect().unwrap();
        assert!(!mgr.should_reconnect()); // 3 failures = limit
    }

    // ── Disconnect ──────────────────────────────────────────────────

    #[test]
    fn disconnect_closes_transport_and_sets_state() {
        let mut transport = MockTransport::with_session(test_session());
        transport.queue_recv(hello_ack(false));

        let mut mgr = RelayConnectionManager::new(test_config(), transport);
        mgr.connect().expect("connect");
        assert_eq!(mgr.state(), ConnectionState::Connected);

        mgr.disconnect();
        assert_eq!(mgr.state(), ConnectionState::Disconnected);
        assert!(mgr.transport.closed);
    }
}
