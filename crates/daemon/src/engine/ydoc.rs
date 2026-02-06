// Y.Doc wrapper using yrs (y-crdt Rust bindings).
// Provides a higher-level API for Scriptum's CRDT operations.

use anyhow::{Context, Result};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, GetString, MapRef, ReadTxn, StateVector, Text, TextRef, Transact, Update};

/// Wrapper around a Yjs document for Scriptum.
pub struct YDoc {
    doc: Doc,
}

impl YDoc {
    /// Create a new empty document.
    pub fn new() -> Self {
        Self { doc: Doc::new() }
    }

    /// Create a document with a specific client ID (for deterministic testing).
    pub fn with_client_id(client_id: u64) -> Self {
        let options = yrs::Options { client_id, ..Default::default() };
        Self { doc: Doc::with_options(options) }
    }

    /// Load a document from a binary state (full snapshot).
    pub fn from_state(data: &[u8]) -> Result<Self> {
        let doc = Doc::new();
        let update = Update::decode_v1(data).context("failed to decode Yjs state")?;
        doc.transact_mut().apply_update(update).context("failed to apply Yjs state update")?;
        Ok(Self { doc })
    }

    /// Apply an incremental binary update to the document.
    pub fn apply_update(&self, data: &[u8]) -> Result<()> {
        let update = Update::decode_v1(data).context("failed to decode Yjs update")?;
        self.doc.transact_mut().apply_update(update).context("failed to apply Yjs update")?;
        Ok(())
    }

    /// Encode the full document state as a binary blob.
    pub fn encode_state(&self) -> Vec<u8> {
        self.doc.transact().encode_state_as_update_v1(&StateVector::default())
    }

    /// Encode the state vector (logical timestamp) for sync protocol.
    pub fn encode_state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// Compute a diff (update) containing all changes since the given state vector.
    pub fn encode_diff(&self, remote_sv: &[u8]) -> Result<Vec<u8>> {
        let sv = StateVector::decode_v1(remote_sv).context("failed to decode state vector")?;
        Ok(self.doc.transact().encode_diff_v1(&sv))
    }

    /// Get or create a `Text` shared type by name.
    pub fn get_or_insert_text(&self, name: &str) -> TextRef {
        self.doc.get_or_insert_text(name)
    }

    /// Get or create a `Map` shared type by name.
    pub fn get_or_insert_map(&self, name: &str) -> MapRef {
        self.doc.get_or_insert_map(name)
    }

    /// Read the string content of a named text type.
    pub fn get_text_string(&self, name: &str) -> String {
        let text = self.doc.get_or_insert_text(name);
        text.get_string(&self.doc.transact())
    }

    /// Insert text at position in a named text type.
    pub fn insert_text(&self, name: &str, index: u32, content: &str) {
        let text = self.doc.get_or_insert_text(name);
        let mut txn = self.doc.transact_mut();
        text.insert(&mut txn, index, content);
    }

    /// Get the underlying Doc reference (for advanced operations).
    pub fn inner(&self) -> &Doc {
        &self.doc
    }
}

impl Default for YDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrs::{Map, Transact};

    #[test]
    fn test_create_new_doc() {
        let doc = YDoc::new();
        assert!(doc.encode_state().len() > 0);
    }

    #[test]
    fn test_text_operations() {
        let doc = YDoc::new();
        doc.insert_text("content", 0, "hello");
        doc.insert_text("content", 5, " world");
        assert_eq!(doc.get_text_string("content"), "hello world");
    }

    #[test]
    fn test_map_operations() {
        let doc = YDoc::new();
        let map = doc.get_or_insert_map("meta");
        {
            let mut txn = doc.inner().transact_mut();
            map.insert(&mut txn, "title", "My Document");
            map.insert(&mut txn, "version", 1i32);
        }
        let txn = doc.inner().transact();
        let title: Option<String> = map.get(&txn, "title").map(|v| v.to_string(&txn));
        assert_eq!(title.as_deref(), Some("My Document"));
    }

    #[test]
    fn test_encode_and_load_state() {
        let doc = YDoc::new();
        doc.insert_text("content", 0, "persistent data");

        let state = doc.encode_state();
        let restored = YDoc::from_state(&state).unwrap();
        assert_eq!(restored.get_text_string("content"), "persistent data");
    }

    #[test]
    fn test_apply_update_sync() {
        let doc_a = YDoc::with_client_id(1);
        let doc_b = YDoc::with_client_id(2);

        doc_a.insert_text("article", 0, "hello");

        // Sync A -> B via state vector + diff
        let sv_b = doc_b.encode_state_vector();
        let diff = doc_a.encode_diff(&sv_b).unwrap();
        doc_b.apply_update(&diff).unwrap();

        assert_eq!(doc_b.get_text_string("article"), "hello");
    }

    #[test]
    fn test_concurrent_edits_merge() {
        let doc_a = YDoc::with_client_id(1);
        let doc_b = YDoc::with_client_id(2);

        // Both start with same text
        doc_a.insert_text("article", 0, "hello");
        let state = doc_a.encode_state();
        doc_b.apply_update(&state).unwrap();

        // Concurrent edits
        doc_a.insert_text("article", 5, " world");
        doc_b.insert_text("article", 0, "Oh, ");

        // Sync both ways
        let sv_b = doc_b.encode_state_vector();
        let diff_a = doc_a.encode_diff(&sv_b).unwrap();
        doc_b.apply_update(&diff_a).unwrap();

        let sv_a = doc_a.encode_state_vector();
        let diff_b = doc_b.encode_diff(&sv_a).unwrap();
        doc_a.apply_update(&diff_b).unwrap();

        // Both should converge
        assert_eq!(doc_a.get_text_string("article"), doc_b.get_text_string("article"));
    }

    #[test]
    fn test_incremental_update() {
        let doc_a = YDoc::with_client_id(1);
        let doc_b = YDoc::with_client_id(2);

        doc_a.insert_text("content", 0, "first");
        let state = doc_a.encode_state();
        doc_b.apply_update(&state).unwrap();

        // Make another edit on A
        doc_a.insert_text("content", 5, " second");

        // Only get the diff since B's state
        let sv_b = doc_b.encode_state_vector();
        let diff = doc_a.encode_diff(&sv_b).unwrap();
        doc_b.apply_update(&diff).unwrap();

        assert_eq!(doc_b.get_text_string("content"), "first second");
    }

    #[test]
    fn test_invalid_update_returns_error() {
        let doc = YDoc::new();
        let result = doc.apply_update(b"not a valid update");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_state_returns_error() {
        let result = YDoc::from_state(b"not a valid state");
        assert!(result.is_err());
    }
}
