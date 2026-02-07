use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use yrs::sync::{Awareness, AwarenessUpdate, DefaultProtocol, Message, Protocol};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PresenceKind {
    Human,
    Agent,
}

impl Default for PresenceKind {
    fn default() -> Self {
        Self::Human
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresenceUser {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: PresenceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CursorPosition {
    pub line: u32,
    pub ch: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionRange {
    pub anchor: CursorPosition,
    pub head: CursorPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresenceState {
    pub user: PresenceUser,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<CursorPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<SelectionRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresencePeer {
    pub client_id: u64,
    pub clock: u32,
    pub presence: PresenceState,
}

#[derive(Debug, Default)]
pub struct AwarenessDispatch {
    pub direct_messages: Vec<Message>,
    pub broadcast_messages: Vec<Message>,
}

#[derive(Debug, Clone, Default)]
pub struct AwarenessProtocolWrapper {
    protocol: DefaultProtocol,
}

impl AwarenessProtocolWrapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_local_presence(
        &self,
        awareness: &Awareness,
        presence: &PresenceState,
    ) -> Result<()> {
        awareness.set_local_state(presence).context("failed to encode local presence state")?;
        Ok(())
    }

    pub fn clear_local_presence(&self, awareness: &Awareness) {
        awareness.clean_local_state();
    }

    pub fn list_peers(&self, awareness: &Awareness) -> Vec<PresencePeer> {
        let mut peers: Vec<PresencePeer> = awareness
            .iter()
            .filter_map(|(client_id, state)| {
                let raw = state.data?;
                let presence: PresenceState = serde_json::from_str(raw.as_ref()).ok()?;
                Some(PresencePeer { client_id, clock: state.clock, presence })
            })
            .collect();

        peers.sort_by_key(|peer| peer.client_id);
        peers
    }

    pub fn handle_message(
        &self,
        awareness: &Awareness,
        message: Message,
    ) -> Result<AwarenessDispatch> {
        match message {
            Message::Awareness(update) => self.handle_awareness_update(awareness, update),
            Message::AwarenessQuery => {
                let direct = self
                    .protocol
                    .handle_awareness_query(awareness)
                    .context("awareness query failed")?;
                Ok(AwarenessDispatch {
                    direct_messages: direct.into_iter().collect(),
                    broadcast_messages: Vec::new(),
                })
            }
            other => {
                let direct = self
                    .protocol
                    .handle_message(awareness, other)
                    .context("awareness protocol message handling failed")?;
                Ok(AwarenessDispatch {
                    direct_messages: direct.into_iter().collect(),
                    broadcast_messages: Vec::new(),
                })
            }
        }
    }

    fn handle_awareness_update(
        &self,
        awareness: &Awareness,
        update: AwarenessUpdate,
    ) -> Result<AwarenessDispatch> {
        let Some(summary) =
            awareness.apply_update_summary(update).context("failed to apply awareness update")?
        else {
            return Ok(AwarenessDispatch::default());
        };

        let changed_clients = summary.all_changes();
        if changed_clients.is_empty() {
            return Ok(AwarenessDispatch::default());
        }

        let rebroadcast = awareness
            .update_with_clients(changed_clients)
            .context("failed to encode awareness rebroadcast payload")?;

        Ok(AwarenessDispatch {
            direct_messages: Vec::new(),
            broadcast_messages: vec![Message::Awareness(rebroadcast)],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AwarenessProtocolWrapper, CursorPosition, PresenceKind, PresenceState, PresenceUser,
        SelectionRange,
    };
    use yrs::sync::{Awareness, Message};
    use yrs::{Doc, Options};

    fn awareness_with_client_id(client_id: u64) -> Awareness {
        let options = Options { client_id, ..Default::default() };
        Awareness::new(Doc::with_options(options))
    }

    fn sample_presence(id: &str, name: &str) -> PresenceState {
        PresenceState {
            user: PresenceUser {
                id: id.to_string(),
                name: name.to_string(),
                kind: PresenceKind::Human,
            },
            cursor: Some(CursorPosition { line: 12, ch: 4 }),
            selection: Some(SelectionRange {
                anchor: CursorPosition { line: 10, ch: 1 },
                head: CursorPosition { line: 12, ch: 4 },
            }),
        }
    }

    #[test]
    fn local_presence_round_trips_as_peer_state() {
        let wrapper = AwarenessProtocolWrapper::new();
        let awareness = awareness_with_client_id(7);
        let alice = sample_presence("user-alice", "Alice");

        wrapper.set_local_presence(&awareness, &alice).expect("local presence should serialize");

        let peers = wrapper.list_peers(&awareness);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].client_id, 7);
        assert_eq!(peers[0].presence, alice);
    }

    #[test]
    fn awareness_update_is_rebroadcast_for_changed_clients() {
        let wrapper = AwarenessProtocolWrapper::new();
        let local = awareness_with_client_id(1);
        let remote = awareness_with_client_id(2);

        wrapper
            .set_local_presence(&remote, &sample_presence("user-bob", "Bob"))
            .expect("remote presence should serialize");
        let update = remote.update().expect("remote update should encode");

        let dispatch = wrapper
            .handle_message(&local, Message::Awareness(update))
            .expect("awareness update should apply");

        assert!(dispatch.direct_messages.is_empty());
        assert_eq!(dispatch.broadcast_messages.len(), 1);
        match &dispatch.broadcast_messages[0] {
            Message::Awareness(rebroadcast) => {
                assert!(rebroadcast.clients.contains_key(&2));
            }
            _ => panic!("expected awareness rebroadcast message"),
        }
    }

    #[test]
    fn stale_awareness_update_is_ignored() {
        let wrapper = AwarenessProtocolWrapper::new();
        let local = awareness_with_client_id(1);
        let remote = awareness_with_client_id(2);

        wrapper
            .set_local_presence(&remote, &sample_presence("user-bob", "Bob"))
            .expect("remote presence should serialize");
        let update = remote.update().expect("remote update should encode");

        let first = wrapper
            .handle_message(&local, Message::Awareness(update.clone()))
            .expect("first update should apply");
        assert_eq!(first.broadcast_messages.len(), 1);

        let second = wrapper
            .handle_message(&local, Message::Awareness(update))
            .expect("second update should apply");
        assert!(second.broadcast_messages.is_empty());
    }

    #[test]
    fn awareness_query_returns_snapshot_message() {
        let wrapper = AwarenessProtocolWrapper::new();
        let awareness = awareness_with_client_id(9);
        wrapper
            .set_local_presence(&awareness, &sample_presence("user-carla", "Carla"))
            .expect("local presence should serialize");

        let dispatch = wrapper
            .handle_message(&awareness, Message::AwarenessQuery)
            .expect("awareness query should succeed");

        assert_eq!(dispatch.direct_messages.len(), 1);
        assert!(dispatch.broadcast_messages.is_empty());
        match &dispatch.direct_messages[0] {
            Message::Awareness(update) => {
                assert!(update.clients.contains_key(&9));
            }
            _ => panic!("expected awareness message as query response"),
        }
    }
}
