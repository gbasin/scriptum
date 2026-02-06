use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::engine::ydoc::YDoc;

pub const DEFAULT_MAX_MEMORY_BYTES: usize = 512 * 1024 * 1024;

struct ManagedDoc {
    doc: Arc<YDoc>,
    subscribers: usize,
    estimated_bytes: usize,
    lru_tick: u64,
}

/// Tracks active document subscriptions and keeps recently-unsubscribed docs in an LRU cache.
pub struct DocManager {
    docs: HashMap<Uuid, ManagedDoc>,
    max_memory_bytes: usize,
    total_memory_bytes: usize,
    next_lru_tick: u64,
}

impl DocManager {
    pub fn new(max_memory_bytes: usize) -> Self {
        Self { docs: HashMap::new(), max_memory_bytes, total_memory_bytes: 0, next_lru_tick: 1 }
    }

    pub fn subscribe_or_create(&mut self, doc_id: Uuid) -> Arc<YDoc> {
        if let Some(entry) = self.docs.get_mut(&doc_id) {
            entry.subscribers = entry.subscribers.saturating_add(1);
            entry.lru_tick = 0;
            return Arc::clone(&entry.doc);
        }

        let doc = Arc::new(YDoc::new());
        let estimated_bytes = estimate_doc_bytes(&doc);
        self.total_memory_bytes = self.total_memory_bytes.saturating_add(estimated_bytes);
        self.docs.insert(
            doc_id,
            ManagedDoc { doc: Arc::clone(&doc), subscribers: 1, estimated_bytes, lru_tick: 0 },
        );
        self.evict_under_pressure();
        doc
    }

    /// Insert a loaded doc with zero subscribers (starts in LRU cache).
    pub fn put_doc(&mut self, doc_id: Uuid, doc: YDoc) -> Arc<YDoc> {
        let arc_doc = Arc::new(doc);
        let estimated_bytes = estimate_doc_bytes(&arc_doc);
        if let Some(previous) = self.docs.remove(&doc_id) {
            self.total_memory_bytes =
                self.total_memory_bytes.saturating_sub(previous.estimated_bytes);
        }

        self.total_memory_bytes = self.total_memory_bytes.saturating_add(estimated_bytes);
        let lru_tick = self.bump_lru_tick();
        self.docs.insert(
            doc_id,
            ManagedDoc { doc: Arc::clone(&arc_doc), subscribers: 0, estimated_bytes, lru_tick },
        );
        self.evict_under_pressure();
        arc_doc
    }

    pub fn unsubscribe(&mut self, doc_id: Uuid) -> bool {
        let became_inactive = {
            let Some(entry) = self.docs.get_mut(&doc_id) else {
                return false;
            };
            if entry.subscribers == 0 {
                return false;
            }

            entry.subscribers -= 1;
            entry.subscribers == 0
        };

        if became_inactive {
            let lru_tick = self.bump_lru_tick();
            if let Some(entry) = self.docs.get_mut(&doc_id) {
                entry.lru_tick = lru_tick;
            }
            self.evict_under_pressure();
        }

        true
    }

    pub fn contains_doc(&self, doc_id: Uuid) -> bool {
        self.docs.contains_key(&doc_id)
    }

    pub fn subscriber_count(&self, doc_id: Uuid) -> usize {
        self.docs.get(&doc_id).map(|entry| entry.subscribers).unwrap_or(0)
    }

    pub fn total_memory_bytes(&self) -> usize {
        self.total_memory_bytes
    }

    pub fn max_memory_bytes(&self) -> usize {
        self.max_memory_bytes
    }

    pub fn cached_lru_doc_ids(&self) -> Vec<Uuid> {
        let mut docs =
            self.docs
                .iter()
                .filter_map(|(doc_id, entry)| {
                    if entry.subscribers == 0 {
                        Some((*doc_id, entry.lru_tick))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
        docs.sort_by_key(|(_, tick)| *tick);
        docs.into_iter().map(|(doc_id, _)| doc_id).collect()
    }

    fn evict_under_pressure(&mut self) {
        while self.total_memory_bytes > self.max_memory_bytes {
            let evict_id = self
                .docs
                .iter()
                .filter_map(|(doc_id, entry)| {
                    if entry.subscribers == 0 {
                        Some((*doc_id, entry.lru_tick))
                    } else {
                        None
                    }
                })
                .min_by_key(|(_, tick)| *tick)
                .map(|(doc_id, _)| doc_id);

            let Some(evict_id) = evict_id else {
                // Under pressure, but all docs are actively subscribed.
                break;
            };

            if let Some(removed) = self.docs.remove(&evict_id) {
                self.total_memory_bytes =
                    self.total_memory_bytes.saturating_sub(removed.estimated_bytes);
            }
        }
    }

    fn bump_lru_tick(&mut self) -> u64 {
        let tick = self.next_lru_tick;
        self.next_lru_tick = self.next_lru_tick.saturating_add(1);
        tick
    }
}

impl Default for DocManager {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_MEMORY_BYTES)
    }
}

fn estimate_doc_bytes(doc: &YDoc) -> usize {
    doc.encode_state().len()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use super::DocManager;
    use crate::engine::ydoc::YDoc;

    #[test]
    fn subscribe_unsubscribe_lifecycle_moves_docs_between_active_and_lru() {
        let mut manager = DocManager::new(1024 * 1024);
        let doc_id = Uuid::new_v4();

        let first = manager.subscribe_or_create(doc_id);
        let second = manager.subscribe_or_create(doc_id);

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(manager.subscriber_count(doc_id), 2);
        assert!(manager.cached_lru_doc_ids().is_empty());

        assert!(manager.unsubscribe(doc_id));
        assert_eq!(manager.subscriber_count(doc_id), 1);
        assert!(manager.cached_lru_doc_ids().is_empty());

        assert!(manager.unsubscribe(doc_id));
        assert_eq!(manager.subscriber_count(doc_id), 0);
        assert_eq!(manager.cached_lru_doc_ids(), vec![doc_id]);

        let third = manager.subscribe_or_create(doc_id);
        assert!(Arc::ptr_eq(&first, &third));
        assert_eq!(manager.subscriber_count(doc_id), 1);
        assert!(manager.cached_lru_doc_ids().is_empty());
    }

    #[test]
    fn evicts_least_recently_used_doc_when_over_memory_threshold() {
        let doc_a = seeded_doc("A", 2048);
        let size_a = doc_a.encode_state().len();
        let doc_b = seeded_doc("B", 2048);
        let size_b = doc_b.encode_state().len();
        let doc_c = seeded_doc("C", 2048);
        let size_c = doc_c.encode_state().len();

        // Keep exactly B + C in memory after eviction.
        let mut manager = DocManager::new(size_b + size_c);

        let doc_a_id = Uuid::new_v4();
        let doc_b_id = Uuid::new_v4();
        let doc_c_id = Uuid::new_v4();

        manager.put_doc(doc_a_id, doc_a);
        manager.put_doc(doc_b_id, doc_b);
        manager.put_doc(doc_c_id, doc_c);

        assert!(!manager.contains_doc(doc_a_id));
        assert!(manager.contains_doc(doc_b_id));
        assert!(manager.contains_doc(doc_c_id));
        assert_eq!(manager.cached_lru_doc_ids(), vec![doc_b_id, doc_c_id]);
        assert!(manager.total_memory_bytes() <= manager.max_memory_bytes());
        assert!(size_a > 0);
    }

    fn seeded_doc(content_unit: &str, repeats: usize) -> YDoc {
        let doc = YDoc::new();
        let content = content_unit.repeat(repeats);
        doc.insert_text("content", 0, &content);
        doc
    }
}
