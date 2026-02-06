// Full-text search: FTS5 index behind abstraction layer.

pub mod backlinks;
pub mod fts;

pub use backlinks::{resolve_wiki_links, BacklinkStore, LinkableDocument, ResolvedBacklink};
pub use fts::{Fts5Index, IndexEntry, SearchHit, SearchIndex};
