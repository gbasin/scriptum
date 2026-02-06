// Backlink target resolution + SQLite persistence.
//
// Resolution order (first match wins):
// 1) Exact path
// 2) Filename (case-insensitive, optional ".md")
// 3) Document title (case-insensitive)

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use scriptum_common::backlink::WikiLink;

/// A candidate document that wiki links can resolve to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkableDocument {
    pub doc_id: String,
    pub path: String,
    pub title: Option<String>,
}

/// A resolved backlink edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBacklink {
    pub source_doc_id: String,
    pub target_doc_id: String,
    /// Raw wiki link text (inner `[[...]]` content).
    pub link_text: String,
}

/// Resolve parsed wiki links to concrete document IDs.
///
/// Resolution order:
/// - exact path (case-sensitive)
/// - filename (case-insensitive, optional `.md`)
/// - title (case-insensitive)
pub fn resolve_wiki_links(
    source_doc_id: &str,
    links: &[WikiLink],
    documents: &[LinkableDocument],
) -> Vec<ResolvedBacklink> {
    links
        .iter()
        .filter_map(|link| {
            let target_doc = resolve_target_doc(&link.target, documents)?;
            Some(ResolvedBacklink {
                source_doc_id: source_doc_id.to_string(),
                target_doc_id: target_doc.doc_id.clone(),
                link_text: link.raw.clone(),
            })
        })
        .collect()
}

fn resolve_target_doc<'a>(
    target: &str,
    documents: &'a [LinkableDocument],
) -> Option<&'a LinkableDocument> {
    let normalized_target_path = normalize_pathish(target);
    if normalized_target_path.is_empty() {
        return None;
    }

    // 1) exact path
    if let Some(doc) =
        documents.iter().find(|doc| normalize_pathish(&doc.path) == normalized_target_path)
    {
        return Some(doc);
    }

    // 2) filename (case-insensitive)
    let target_filename_key = filename_key(&normalized_target_path);
    if let Some(doc) = documents
        .iter()
        .find(|doc| filename_key(&normalize_pathish(&doc.path)) == target_filename_key)
    {
        return Some(doc);
    }

    // 3) title (case-insensitive)
    let target_title_key = target.trim().to_ascii_lowercase();
    documents.iter().find(|doc| {
        doc.title
            .as_deref()
            .map(|title| title.trim().to_ascii_lowercase() == target_title_key)
            .unwrap_or(false)
    })
}

fn normalize_pathish(value: &str) -> String {
    value.trim().replace('\\', "/").trim_matches('/').to_string()
}

fn filename_key(pathish: &str) -> String {
    let file = pathish.rsplit('/').next().unwrap_or(pathish);
    file.strip_suffix(".md")
        .or_else(|| file.strip_suffix(".MD"))
        .unwrap_or(file)
        .to_ascii_lowercase()
}

/// SQLite-backed backlink persistence.
pub struct BacklinkStore<'a> {
    conn: &'a Connection,
}

impl<'a> BacklinkStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Ensure backlinks schema exists in the current database.
    pub fn ensure_schema(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS backlinks (
                    source_doc_id    TEXT NOT NULL,
                    target_doc_id    TEXT NOT NULL,
                    link_text        TEXT NOT NULL,
                    PRIMARY KEY (source_doc_id, target_doc_id, link_text)
                );
                CREATE INDEX IF NOT EXISTS backlinks_target_idx
                    ON backlinks (target_doc_id);",
            )
            .context("failed to ensure backlinks schema")?;
        Ok(())
    }

    /// Replace all backlinks for a source document.
    pub fn replace_for_source(
        &self,
        source_doc_id: &str,
        backlinks: &[ResolvedBacklink],
    ) -> Result<()> {
        let tx = self
            .conn
            .unchecked_transaction()
            .context("failed to start backlinks replacement transaction")?;

        tx.execute("DELETE FROM backlinks WHERE source_doc_id = ?1", params![source_doc_id])
            .context("failed to clear backlinks for source doc")?;

        for backlink in backlinks {
            tx.execute(
                "INSERT OR IGNORE INTO backlinks (source_doc_id, target_doc_id, link_text)
                 VALUES (?1, ?2, ?3)",
                params![backlink.source_doc_id, backlink.target_doc_id, backlink.link_text],
            )
            .context("failed to insert resolved backlink")?;
        }

        tx.commit().context("failed to commit backlinks replacement transaction")?;
        Ok(())
    }

    /// Incoming backlinks for a target document.
    pub fn incoming_for_target(&self, target_doc_id: &str) -> Result<Vec<ResolvedBacklink>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT source_doc_id, target_doc_id, link_text
                 FROM backlinks
                 WHERE target_doc_id = ?1
                 ORDER BY source_doc_id, link_text",
            )
            .context("failed to prepare incoming backlinks query")?;

        let rows = stmt
            .query_map(params![target_doc_id], |row| {
                Ok(ResolvedBacklink {
                    source_doc_id: row.get(0)?,
                    target_doc_id: row.get(1)?,
                    link_text: row.get(2)?,
                })
            })
            .context("failed to query incoming backlinks")?;

        rows.collect::<rusqlite::Result<Vec<_>>>().context("failed to collect incoming backlinks")
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use scriptum_common::backlink::parse_wiki_links;

    use super::{resolve_wiki_links, BacklinkStore, LinkableDocument, ResolvedBacklink};

    fn docs() -> Vec<LinkableDocument> {
        vec![
            LinkableDocument {
                doc_id: "doc-a".into(),
                path: "docs/auth.md".into(),
                title: Some("Authentication".into()),
            },
            LinkableDocument {
                doc_id: "doc-b".into(),
                path: "guides/AUTH.md".into(),
                title: Some("Auth Guide".into()),
            },
            LinkableDocument {
                doc_id: "doc-c".into(),
                path: "notes/security.md".into(),
                title: Some("Security Notes".into()),
            },
        ]
    }

    #[test]
    fn resolves_exact_path_before_other_strategies() {
        let links = parse_wiki_links("See [[docs/auth.md]].");
        let resolved = resolve_wiki_links("source-1", &links, &docs());

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].target_doc_id, "doc-a");
    }

    #[test]
    fn resolves_by_filename_case_insensitive_when_exact_path_not_found() {
        let links = parse_wiki_links("See [[auth]].");
        let resolved = resolve_wiki_links("source-1", &links, &docs());

        assert_eq!(resolved.len(), 1);
        // doc-a appears first in the candidate list, so first match wins.
        assert_eq!(resolved[0].target_doc_id, "doc-a");
    }

    #[test]
    fn resolves_by_title_as_final_fallback() {
        let links = parse_wiki_links("See [[Security Notes]].");
        let resolved = resolve_wiki_links("source-1", &links, &docs());

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].target_doc_id, "doc-c");
    }

    #[test]
    fn uses_first_match_wins_ordering() {
        let links = parse_wiki_links("See [[auth]].");
        let candidates = vec![
            LinkableDocument {
                doc_id: "first".into(),
                path: "x/auth.md".into(),
                title: Some("Auth X".into()),
            },
            LinkableDocument {
                doc_id: "second".into(),
                path: "y/auth.md".into(),
                title: Some("Auth Y".into()),
            },
        ];

        let resolved = resolve_wiki_links("source-1", &links, &candidates);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].target_doc_id, "first");
    }

    #[test]
    fn unresolved_links_are_skipped() {
        let links = parse_wiki_links("See [[does-not-exist]].");
        let resolved = resolve_wiki_links("source-1", &links, &docs());
        assert!(resolved.is_empty());
    }

    #[test]
    fn stores_and_replaces_backlinks_in_sqlite_table() {
        let conn = Connection::open_in_memory().expect("in-memory sqlite should open");
        let store = BacklinkStore::new(&conn);
        store.ensure_schema().expect("backlinks schema should be ensured");

        let first = vec![
            ResolvedBacklink {
                source_doc_id: "source-1".into(),
                target_doc_id: "doc-a".into(),
                link_text: "auth".into(),
            },
            ResolvedBacklink {
                source_doc_id: "source-1".into(),
                target_doc_id: "doc-c".into(),
                link_text: "Security Notes".into(),
            },
        ];
        store.replace_for_source("source-1", &first).expect("initial backlinks should store");

        let incoming_a =
            store.incoming_for_target("doc-a").expect("incoming backlinks should query");
        assert_eq!(incoming_a.len(), 1);
        assert_eq!(incoming_a[0].source_doc_id, "source-1");
        assert_eq!(incoming_a[0].link_text, "auth");

        // Replacement should delete old rows for this source before inserting new rows.
        let second = vec![ResolvedBacklink {
            source_doc_id: "source-1".into(),
            target_doc_id: "doc-b".into(),
            link_text: "Auth Guide".into(),
        }];
        store.replace_for_source("source-1", &second).expect("replacement backlinks should store");

        assert!(store
            .incoming_for_target("doc-a")
            .expect("incoming backlinks should query")
            .is_empty());
        let incoming_b =
            store.incoming_for_target("doc-b").expect("incoming backlinks should query");
        assert_eq!(incoming_b.len(), 1);
        assert_eq!(incoming_b[0].link_text, "Auth Guide");
    }
}
