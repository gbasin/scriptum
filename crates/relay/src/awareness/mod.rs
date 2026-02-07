// Awareness aggregation (presence, cursors, and section claims).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Tracks per-doc awareness (cursors, presence, names) from each session.
///
/// Key: (workspace_id, doc_id, session_id)
/// Value: list of peer awareness entries from that session.
#[derive(Debug, Clone, Default)]
pub struct AwarenessStore {
    state: Arc<RwLock<HashMap<(Uuid, Uuid), HashMap<Uuid, Vec<serde_json::Value>>>>>,
}

impl AwarenessStore {
    /// Update awareness for a session's contribution to a doc.
    pub async fn update(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        session_id: Uuid,
        peers: Vec<serde_json::Value>,
    ) {
        let mut guard = self.state.write().await;
        let doc_state = guard.entry((workspace_id, doc_id)).or_default();
        if peers.is_empty() {
            doc_state.remove(&session_id);
        } else {
            doc_state.insert(session_id, peers);
        }
    }

    /// Remove all awareness for a session (on disconnect).
    pub async fn remove_session(&self, workspace_id: Uuid, doc_ids: &[Uuid], session_id: Uuid) {
        let mut guard = self.state.write().await;
        for doc_id in doc_ids {
            if let Some(doc_state) = guard.get_mut(&(workspace_id, *doc_id)) {
                doc_state.remove(&session_id);
                if doc_state.is_empty() {
                    guard.remove(&(workspace_id, *doc_id));
                }
            }
        }
    }

    /// Get aggregated awareness for a doc (all sessions' peers merged).
    pub async fn aggregate(&self, workspace_id: Uuid, doc_id: Uuid) -> Vec<serde_json::Value> {
        let guard = self.state.read().await;
        guard
            .get(&(workspace_id, doc_id))
            .map(|doc_state| doc_state.values().flatten().cloned().collect())
            .unwrap_or_default()
    }

    /// Get aggregated awareness excluding a specific session (for broadcast).
    pub async fn aggregate_excluding(
        &self,
        workspace_id: Uuid,
        doc_id: Uuid,
        exclude_session: Uuid,
    ) -> Vec<serde_json::Value> {
        let guard = self.state.read().await;
        guard
            .get(&(workspace_id, doc_id))
            .map(|doc_state| {
                doc_state
                    .iter()
                    .filter(|(sid, _)| **sid != exclude_session)
                    .flat_map(|(_, peers)| peers.iter().cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get typed peer states for a document by parsing raw JSON values.
    pub async fn peers_for_doc(&self, workspace_id: Uuid, doc_id: Uuid) -> Vec<PeerState> {
        let raw = self.aggregate(workspace_id, doc_id).await;
        raw.into_iter().filter_map(|v| PeerState::from_json(&v)).collect()
    }

    /// Return a snapshot of all documents with active peers in a workspace.
    pub async fn who_is_where(&self, workspace_id: Uuid) -> Vec<DocPresence> {
        let guard = self.state.read().await;
        let mut results = Vec::new();
        for ((ws_id, doc_id), sessions) in guard.iter() {
            if *ws_id != workspace_id {
                continue;
            }
            let peers: Vec<PeerState> =
                sessions.values().flatten().filter_map(|v| PeerState::from_json(v)).collect();
            if !peers.is_empty() {
                results.push(DocPresence { doc_id: *doc_id, peers });
            }
        }
        results
    }

    /// Count active sessions across all documents in a workspace.
    pub async fn active_session_count(&self, workspace_id: Uuid) -> usize {
        let guard = self.state.read().await;
        let mut sessions = std::collections::HashSet::<Uuid>::new();
        for ((ws_id, _), doc_sessions) in guard.iter() {
            if *ws_id == workspace_id {
                sessions.extend(doc_sessions.keys());
            }
        }
        sessions.len()
    }

    /// List all document IDs that have active awareness in a workspace.
    pub async fn active_docs(&self, workspace_id: Uuid) -> Vec<Uuid> {
        let guard = self.state.read().await;
        guard
            .keys()
            .filter(|(ws_id, _)| *ws_id == workspace_id)
            .map(|(_, doc_id)| *doc_id)
            .collect()
    }
}

/// A typed, structured view of a single peer's awareness state.
///
/// Parsed from the raw `serde_json::Value` sent by clients.
/// Unknown fields are silently ignored so older clients still work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerState {
    /// Display name of this peer.
    pub name: String,
    /// Hex color assigned to this peer (e.g. "#e06c75").
    #[serde(default)]
    pub color: Option<String>,
    /// Absolute cursor offset in the document.
    #[serde(default)]
    pub cursor: Option<u32>,
    /// Selection range (anchor, head) — `None` when collapsed to cursor.
    #[serde(default)]
    pub selection: Option<SelectionRange>,
    /// Whether this peer is a human or an agent.
    #[serde(default)]
    pub editor_type: Option<EditorKind>,
    /// Agent ID if `editor_type` is `Agent`.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Advisory section lease claims from this peer.
    #[serde(default)]
    pub claimed_sections: Vec<String>,
    /// Timestamp of last cursor/edit activity.
    #[serde(default)]
    pub last_active_at: Option<DateTime<Utc>>,
}

impl PeerState {
    /// Try to parse a `PeerState` from a raw JSON value.
    /// Returns `None` if the value lacks the required `name` field.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

/// Cursor selection range.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionRange {
    /// Anchor position (selection start).
    pub anchor: u32,
    /// Head position (selection end / caret).
    pub head: u32,
}

/// Whether a peer is a human editor or an agent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditorKind {
    Human,
    Agent,
}

/// Presence snapshot for a single document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocPresence {
    pub doc_id: Uuid,
    pub peers: Vec<PeerState>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ws_id() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
    }

    fn doc_a() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-00000000000a").unwrap()
    }

    fn doc_b() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-00000000000b").unwrap()
    }

    fn session_1() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000011").unwrap()
    }

    fn session_2() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000012").unwrap()
    }

    fn session_3() -> Uuid {
        Uuid::parse_str("00000000-0000-0000-0000-000000000013").unwrap()
    }

    fn alice_peer() -> serde_json::Value {
        json!({
            "name": "Alice",
            "color": "#e06c75",
            "cursor": 42,
            "editor_type": "human",
            "claimed_sections": ["root/intro"]
        })
    }

    fn bob_peer() -> serde_json::Value {
        json!({
            "name": "Bob",
            "color": "#61afef",
            "cursor": 100,
            "selection": { "anchor": 90, "head": 110 },
            "editor_type": "human"
        })
    }

    fn agent_peer() -> serde_json::Value {
        json!({
            "name": "claude-agent",
            "editor_type": "agent",
            "agent_id": "agent-001",
            "cursor": 200,
            "claimed_sections": ["root/api", "root/api/auth"]
        })
    }

    // ── PeerState parsing ──────────────────────────────────────────

    #[test]
    fn peer_state_from_json_parses_human() {
        let state = PeerState::from_json(&alice_peer()).unwrap();
        assert_eq!(state.name, "Alice");
        assert_eq!(state.color.as_deref(), Some("#e06c75"));
        assert_eq!(state.cursor, Some(42));
        assert_eq!(state.editor_type, Some(EditorKind::Human));
        assert_eq!(state.claimed_sections, vec!["root/intro"]);
        assert!(state.selection.is_none());
        assert!(state.agent_id.is_none());
    }

    #[test]
    fn peer_state_from_json_parses_agent() {
        let state = PeerState::from_json(&agent_peer()).unwrap();
        assert_eq!(state.name, "claude-agent");
        assert_eq!(state.editor_type, Some(EditorKind::Agent));
        assert_eq!(state.agent_id.as_deref(), Some("agent-001"));
        assert_eq!(state.claimed_sections, vec!["root/api", "root/api/auth"]);
    }

    #[test]
    fn peer_state_from_json_parses_selection() {
        let state = PeerState::from_json(&bob_peer()).unwrap();
        let sel = state.selection.unwrap();
        assert_eq!(sel.anchor, 90);
        assert_eq!(sel.head, 110);
    }

    #[test]
    fn peer_state_from_json_returns_none_for_missing_name() {
        let bad = json!({"cursor": 10});
        assert!(PeerState::from_json(&bad).is_none());
    }

    #[test]
    fn peer_state_from_json_tolerates_unknown_fields() {
        let extended = json!({
            "name": "Future",
            "some_new_field": true,
            "nested": {"deep": 1}
        });
        let state = PeerState::from_json(&extended).unwrap();
        assert_eq!(state.name, "Future");
    }

    #[test]
    fn peer_state_roundtrip_json() {
        let state = PeerState {
            name: "Alice".into(),
            color: Some("#e06c75".into()),
            cursor: Some(42),
            selection: Some(SelectionRange { anchor: 10, head: 20 }),
            editor_type: Some(EditorKind::Human),
            agent_id: None,
            claimed_sections: vec!["root/intro".into()],
            last_active_at: None,
        };
        let json = serde_json::to_value(&state).unwrap();
        let parsed = PeerState::from_json(&json).unwrap();
        assert_eq!(state, parsed);
    }

    // ── AwarenessStore basic operations ────────────────────────────

    #[tokio::test]
    async fn update_stores_and_aggregate_returns_all() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_a(), session_2(), vec![bob_peer()]).await;

        let all = store.aggregate(ws_id(), doc_a()).await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn aggregate_empty_doc_returns_empty() {
        let store = AwarenessStore::default();
        let all = store.aggregate(ws_id(), doc_a()).await;
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn update_with_empty_peers_removes_session() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_a(), session_1(), vec![]).await;

        let all = store.aggregate(ws_id(), doc_a()).await;
        assert!(all.is_empty());
    }

    #[tokio::test]
    async fn aggregate_excluding_omits_sender() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_a(), session_2(), vec![bob_peer()]).await;

        let without_1 = store.aggregate_excluding(ws_id(), doc_a(), session_1()).await;
        assert_eq!(without_1.len(), 1);
        assert_eq!(without_1[0]["name"], "Bob");
    }

    #[tokio::test]
    async fn remove_session_clears_all_subscribed_docs() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_b(), session_1(), vec![alice_peer()]).await;

        store.remove_session(ws_id(), &[doc_a(), doc_b()], session_1()).await;

        assert!(store.aggregate(ws_id(), doc_a()).await.is_empty());
        assert!(store.aggregate(ws_id(), doc_b()).await.is_empty());
    }

    #[tokio::test]
    async fn remove_session_preserves_other_sessions() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_a(), session_2(), vec![bob_peer()]).await;

        store.remove_session(ws_id(), &[doc_a()], session_1()).await;

        let remaining = store.aggregate(ws_id(), doc_a()).await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0]["name"], "Bob");
    }

    // ── Typed aggregation (peers_for_doc) ──────────────────────────

    #[tokio::test]
    async fn peers_for_doc_returns_typed_states() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_a(), session_2(), vec![agent_peer()]).await;

        let peers = store.peers_for_doc(ws_id(), doc_a()).await;
        assert_eq!(peers.len(), 2);

        let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"claude-agent"));
    }

    #[tokio::test]
    async fn peers_for_doc_skips_unparseable_entries() {
        let store = AwarenessStore::default();
        // Valid peer + invalid peer (no name field).
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer(), json!({"cursor": 5})]).await;

        let peers = store.peers_for_doc(ws_id(), doc_a()).await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "Alice");
    }

    // ── Who-is-where ───────────────────────────────────────────────

    #[tokio::test]
    async fn who_is_where_returns_all_active_docs() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_b(), session_2(), vec![bob_peer()]).await;

        let presence = store.who_is_where(ws_id()).await;
        assert_eq!(presence.len(), 2);

        let doc_ids: Vec<Uuid> = presence.iter().map(|p| p.doc_id).collect();
        assert!(doc_ids.contains(&doc_a()));
        assert!(doc_ids.contains(&doc_b()));
    }

    #[tokio::test]
    async fn who_is_where_excludes_other_workspaces() {
        let store = AwarenessStore::default();
        let other_ws = Uuid::parse_str("00000000-0000-0000-0000-000000000099").unwrap();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(other_ws, doc_b(), session_2(), vec![bob_peer()]).await;

        let presence = store.who_is_where(ws_id()).await;
        assert_eq!(presence.len(), 1);
        assert_eq!(presence[0].doc_id, doc_a());
    }

    #[tokio::test]
    async fn who_is_where_skips_docs_with_no_parseable_peers() {
        let store = AwarenessStore::default();
        // Only invalid peer (no name).
        store.update(ws_id(), doc_a(), session_1(), vec![json!({"cursor": 5})]).await;

        let presence = store.who_is_where(ws_id()).await;
        assert!(presence.is_empty());
    }

    // ── Active session count ───────────────────────────────────────

    #[tokio::test]
    async fn active_session_count_deduplicates_across_docs() {
        let store = AwarenessStore::default();
        // session_1 active in both doc_a and doc_b.
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_b(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_a(), session_2(), vec![bob_peer()]).await;

        // 2 unique sessions, even though session_1 appears twice.
        assert_eq!(store.active_session_count(ws_id()).await, 2);
    }

    #[tokio::test]
    async fn active_session_count_excludes_other_workspaces() {
        let store = AwarenessStore::default();
        let other_ws = Uuid::parse_str("00000000-0000-0000-0000-000000000099").unwrap();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(other_ws, doc_a(), session_2(), vec![bob_peer()]).await;

        assert_eq!(store.active_session_count(ws_id()).await, 1);
    }

    // ── Active docs ────────────────────────────────────────────────

    #[tokio::test]
    async fn active_docs_lists_all_docs_with_presence() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_b(), session_2(), vec![bob_peer()]).await;

        let docs = store.active_docs(ws_id()).await;
        assert_eq!(docs.len(), 2);
        assert!(docs.contains(&doc_a()));
        assert!(docs.contains(&doc_b()));
    }

    #[tokio::test]
    async fn active_docs_empty_when_no_presence() {
        let store = AwarenessStore::default();
        let docs = store.active_docs(ws_id()).await;
        assert!(docs.is_empty());
    }

    // ── Multi-session scenario ─────────────────────────────────────

    #[tokio::test]
    async fn multi_session_aggregate_and_disconnect() {
        let store = AwarenessStore::default();
        store.update(ws_id(), doc_a(), session_1(), vec![alice_peer()]).await;
        store.update(ws_id(), doc_a(), session_2(), vec![bob_peer()]).await;
        store.update(ws_id(), doc_a(), session_3(), vec![agent_peer()]).await;

        // All three present.
        assert_eq!(store.peers_for_doc(ws_id(), doc_a()).await.len(), 3);

        // Session 2 disconnects.
        store.remove_session(ws_id(), &[doc_a()], session_2()).await;
        let peers = store.peers_for_doc(ws_id(), doc_a()).await;
        assert_eq!(peers.len(), 2);
        let names: Vec<&str> = peers.iter().map(|p| p.name.as_str()).collect();
        assert!(!names.contains(&"Bob"));
        assert!(names.contains(&"Alice"));
        assert!(names.contains(&"claude-agent"));

        // Session 1 disconnects.
        store.remove_session(ws_id(), &[doc_a()], session_1()).await;
        let peers = store.peers_for_doc(ws_id(), doc_a()).await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "claude-agent");
    }
}
