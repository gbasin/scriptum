// Full-text search: FTS5 index behind abstraction layer.

pub mod fts;

pub use fts::{Fts5Index, IndexEntry, SearchHit, SearchIndex};
