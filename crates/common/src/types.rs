// Core domain types shared across all Scriptum crates.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A workspace is a top-level container for related documents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    /// Caller's role in this workspace (if applicable).
    pub role: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub etag: String,
}

/// A document within a workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Document {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub path: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub head_seq: i64,
    pub etag: String,
    pub archived_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A heading-based section within a document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Section {
    /// Stable ID: ancestor slug chain, e.g. "root/api/authentication" or "h2_3" fallback.
    pub id: String,
    /// Parent section ID (forms a tree via heading levels).
    pub parent_id: Option<String>,
    /// The heading text, e.g. "## Authentication".
    pub heading: String,
    /// Heading level (1-6).
    pub level: u8,
    /// Start line in the document (1-based).
    pub start_line: u32,
    /// End line in the document (1-based, exclusive).
    pub end_line: u32,
}

/// Detected overlap: multiple editors in the same section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SectionOverlap {
    pub section: Section,
    pub editors: Vec<OverlapEditor>,
    /// `info` = same section, `warning` = same paragraph.
    pub severity: OverlapSeverity,
}

/// An editor involved in a section overlap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverlapEditor {
    pub name: String,
    pub editor_type: EditorType,
    /// Cursor offset within the section.
    pub cursor_offset: u32,
    pub last_edit_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditorType {
    Human,
    Agent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlapSeverity {
    Info,
    Warning,
}

/// An active agent session as returned by `agent.status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSession {
    pub agent_id: String,
    pub workspace_id: Uuid,
    pub last_seen_at: DateTime<Utc>,
    pub active_sections: u32,
}
