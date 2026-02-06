// Section awareness: heading tree rebuild and diff on CRDT updates.
//
// On each document update the markdown is re-parsed into a section tree
// (via scriptum_common::section::parser) and diffed against the previous
// tree. Changes — added, removed, or modified sections — are returned
// for downstream consumers (overlap detection, attribution, notifications).
//
// `SectionTracker` is the per-document entry point. Call `update()` each
// time the CRDT text changes; it returns a `SectionDiff` describing what
// moved. The tracker also maintains per-section `last_edited_by` state,
// updated from the Yjs awareness origin passed into `update()`.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use scriptum_common::section::parser::parse_sections;
use scriptum_common::types::Section;

// ── Change types ────────────────────────────────────────────────────

/// A single change detected between two section tree parses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionChange {
    /// A section appeared that did not exist before.
    Added(Section),
    /// A section that existed before is no longer present.
    Removed(Section),
    /// A section exists in both trees but its content or metadata changed.
    Modified { old: Section, new: Section },
}

/// The result of diffing two section trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionDiff {
    pub changes: Vec<SectionChange>,
}

impl SectionDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn added(&self) -> impl Iterator<Item = &Section> {
        self.changes.iter().filter_map(|c| match c {
            SectionChange::Added(s) => Some(s),
            _ => None,
        })
    }

    pub fn removed(&self) -> impl Iterator<Item = &Section> {
        self.changes.iter().filter_map(|c| match c {
            SectionChange::Removed(s) => Some(s),
            _ => None,
        })
    }

    pub fn modified(&self) -> impl Iterator<Item = (&Section, &Section)> {
        self.changes.iter().filter_map(|c| match c {
            SectionChange::Modified { old, new } => Some((old, new)),
            _ => None,
        })
    }
}

// ── Diff algorithm ──────────────────────────────────────────────────

/// Diff two section lists, matching by section ID.
///
/// Content hashes are computed from the markdown text so that sections
/// whose line range shifted (due to edits above them) but whose actual
/// content is identical are NOT reported as modified.
pub fn diff_sections(old: &[Section], old_md: &str, new: &[Section], new_md: &str) -> SectionDiff {
    let old_hashes = content_hashes(old_md, old);
    let new_hashes = content_hashes(new_md, new);

    let old_map: HashMap<&str, (usize, &Section)> =
        old.iter().enumerate().map(|(i, s)| (s.id.as_str(), (i, s))).collect();
    let new_map: HashMap<&str, (usize, &Section)> =
        new.iter().enumerate().map(|(i, s)| (s.id.as_str(), (i, s))).collect();

    let mut changes = Vec::new();

    // Removed + modified (iterate old, look up in new).
    for (idx, old_section) in old.iter().enumerate() {
        match new_map.get(old_section.id.as_str()) {
            None => changes.push(SectionChange::Removed(old_section.clone())),
            Some(&(new_idx, new_section)) => {
                if section_metadata_differs(old_section, new_section)
                    || old_hashes[idx] != new_hashes[new_idx]
                {
                    changes.push(SectionChange::Modified {
                        old: old_section.clone(),
                        new: new_section.clone(),
                    });
                }
            }
        }
    }

    // Added (in new but not in old).
    for new_section in new {
        if !old_map.contains_key(new_section.id.as_str()) {
            changes.push(SectionChange::Added(new_section.clone()));
        }
    }

    SectionDiff { changes }
}

/// Metadata comparison (heading text, level, parent_id).
/// Line numbers are NOT compared here — content hashing handles that.
fn section_metadata_differs(a: &Section, b: &Section) -> bool {
    a.heading != b.heading || a.level != b.level || a.parent_id != b.parent_id
}

/// Compute a content hash for each section by hashing the markdown lines
/// within its [start_line, end_line) range.
fn content_hashes(markdown: &str, sections: &[Section]) -> Vec<u64> {
    if sections.is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = markdown.lines().collect();

    sections
        .iter()
        .map(|section| {
            let mut hasher = DefaultHasher::new();
            let start = (section.start_line as usize).saturating_sub(1);
            let end = (section.end_line as usize).saturating_sub(1).min(lines.len());
            for line in &lines[start..end] {
                line.hash(&mut hasher);
            }
            hasher.finish()
        })
        .collect()
}

// ── Tracker ─────────────────────────────────────────────────────────

/// Per-document section tracker.
///
/// Holds the most recent section tree and markdown content. On each
/// `update()` call it re-parses the markdown, diffs against the previous
/// tree, maintains `last_edited_by` attribution, and returns the diff.
pub struct SectionTracker {
    sections: Vec<Section>,
    markdown: String,
    last_edited_by: HashMap<String, String>,
}

impl SectionTracker {
    pub fn new() -> Self {
        Self { sections: Vec::new(), markdown: String::new(), last_edited_by: HashMap::new() }
    }

    /// Rebuild the section tree from updated markdown and return the diff.
    ///
    /// `edited_by` — name of the editor that triggered this update (from
    /// Yjs awareness origin). If provided, all added/modified sections
    /// will have their `last_edited_by` updated.
    pub fn update(&mut self, markdown: &str, edited_by: Option<&str>) -> SectionDiff {
        let new_sections = parse_sections(markdown);
        let diff = diff_sections(&self.sections, &self.markdown, &new_sections, markdown);

        if let Some(editor) = edited_by {
            for change in &diff.changes {
                match change {
                    SectionChange::Added(s) => {
                        self.last_edited_by.insert(s.id.clone(), editor.to_string());
                    }
                    SectionChange::Modified { new, .. } => {
                        self.last_edited_by.insert(new.id.clone(), editor.to_string());
                    }
                    SectionChange::Removed(s) => {
                        self.last_edited_by.remove(&s.id);
                    }
                }
            }
        }

        self.sections = new_sections;
        self.markdown = markdown.to_string();
        diff
    }

    /// The current section tree.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Look up the last editor for a section by ID.
    pub fn last_edited_by(&self, section_id: &str) -> Option<&str> {
        self.last_edited_by.get(section_id).map(|s| s.as_str())
    }
}

impl Default for SectionTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── diff_sections ───────────────────────────────────────────────

    #[test]
    fn empty_to_empty_produces_no_changes() {
        let diff = diff_sections(&[], "", &[], "");
        assert!(diff.is_empty());
    }

    #[test]
    fn empty_to_sections_produces_only_adds() {
        let md = "# Hello\n\n## World\n";
        let sections = parse_sections(md);
        let diff = diff_sections(&[], "", &sections, md);

        assert_eq!(diff.added().count(), 2);
        assert_eq!(diff.removed().count(), 0);
        assert_eq!(diff.modified().count(), 0);
    }

    #[test]
    fn sections_to_empty_produces_only_removes() {
        let md = "# Hello\n\n## World\n";
        let sections = parse_sections(md);
        let diff = diff_sections(&sections, md, &[], "");

        assert_eq!(diff.added().count(), 0);
        assert_eq!(diff.removed().count(), 2);
        assert_eq!(diff.modified().count(), 0);
    }

    #[test]
    fn identical_markdown_produces_no_changes() {
        let md = "# Root\n\nSome text.\n\n## Child\n\nMore text.\n";
        let sections = parse_sections(md);
        let diff = diff_sections(&sections, md, &sections, md);

        assert!(diff.is_empty());
    }

    #[test]
    fn adding_a_section_is_detected() {
        let old_md = "# Root\n\nSome text.\n";
        let new_md = "# Root\n\nSome text.\n\n## Added\n\nNew content.\n";
        let old = parse_sections(old_md);
        let new = parse_sections(new_md);
        let diff = diff_sections(&old, old_md, &new, new_md);

        assert_eq!(diff.added().count(), 1);
        let added: Vec<_> = diff.added().collect();
        assert_eq!(added[0].heading, "Added");
    }

    #[test]
    fn removing_a_section_is_detected() {
        let old_md = "# Root\n\n## ToRemove\n\nGone soon.\n\n## Keep\n\nStays.\n";
        let new_md = "# Root\n\n## Keep\n\nStays.\n";
        let old = parse_sections(old_md);
        let new = parse_sections(new_md);
        let diff = diff_sections(&old, old_md, &new, new_md);

        let removed: Vec<_> = diff.removed().collect();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].heading, "ToRemove");
    }

    #[test]
    fn content_change_within_section_is_detected() {
        let old_md = "# Root\n\nOriginal content.\n";
        let new_md = "# Root\n\nModified content here.\n";
        let old = parse_sections(old_md);
        let new = parse_sections(new_md);
        let diff = diff_sections(&old, old_md, &new, new_md);

        let modified: Vec<_> = diff.modified().collect();
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].0.heading, "Root");
    }

    #[test]
    fn heading_text_change_is_detected_as_add_and_remove() {
        // Changing heading text changes the section ID, so it shows as
        // remove old + add new (not as modified).
        let old_md = "# Old Title\n\nContent.\n";
        let new_md = "# New Title\n\nContent.\n";
        let old = parse_sections(old_md);
        let new = parse_sections(new_md);
        let diff = diff_sections(&old, old_md, &new, new_md);

        assert_eq!(diff.removed().count(), 1);
        assert_eq!(diff.added().count(), 1);
        assert_eq!(diff.removed().next().unwrap().heading, "Old Title");
        assert_eq!(diff.added().next().unwrap().heading, "New Title");
    }

    #[test]
    fn shifted_lines_without_content_change_not_reported() {
        // Insert a blank line before a section — its line numbers change
        // but its content is the same.
        let old_md = "# Root\n\n## Child\n\nContent stays same.\n";
        let new_md = "# Root\n\nNew paragraph above.\n\n## Child\n\nContent stays same.\n";
        let old = parse_sections(old_md);
        let new = parse_sections(new_md);
        let diff = diff_sections(&old, old_md, &new, new_md);

        // Root section content changed (new paragraph added).
        // Child section shifted down but content is the same → only Root modified.
        let modified: Vec<_> = diff.modified().collect();
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0].0.heading, "Root");
    }

    #[test]
    fn multiple_changes_in_one_diff() {
        let old_md = "# Root\n\n## A\n\nAlpha.\n\n## B\n\nBeta.\n\n## C\n\nGamma.\n";
        let new_md = "# Root\n\n## A\n\nAlpha changed.\n\n## D\n\nDelta.\n";
        let old = parse_sections(old_md);
        let new = parse_sections(new_md);
        let diff = diff_sections(&old, old_md, &new, new_md);

        // B removed, C removed, A modified (content), D added
        // Root is also modified because its end_line content changed
        let removed: Vec<_> = diff.removed().map(|s| s.heading.as_str()).collect();
        let added: Vec<_> = diff.added().map(|s| s.heading.as_str()).collect();

        assert!(removed.contains(&"B"));
        assert!(removed.contains(&"C"));
        assert!(added.contains(&"D"));
    }

    // ── SectionTracker ──────────────────────────────────────────────

    #[test]
    fn tracker_initial_update_returns_all_sections_as_added() {
        let mut tracker = SectionTracker::new();
        let md = "# Title\n\n## Intro\n\nHello.\n";
        let diff = tracker.update(md, None);

        assert_eq!(diff.added().count(), 2);
        assert_eq!(tracker.sections().len(), 2);
    }

    #[test]
    fn tracker_identical_update_returns_empty_diff() {
        let mut tracker = SectionTracker::new();
        let md = "# Title\n\nContent.\n";

        tracker.update(md, None);
        let diff = tracker.update(md, None);

        assert!(diff.is_empty());
    }

    #[test]
    fn tracker_tracks_last_edited_by() {
        let mut tracker = SectionTracker::new();

        let md1 = "# Root\n\nOriginal.\n";
        tracker.update(md1, Some("alice"));
        assert_eq!(tracker.last_edited_by("root"), Some("alice"));

        let md2 = "# Root\n\nEdited by bob.\n";
        tracker.update(md2, Some("bob"));
        assert_eq!(tracker.last_edited_by("root"), Some("bob"));
    }

    #[test]
    fn tracker_clears_attribution_on_section_removal() {
        let mut tracker = SectionTracker::new();

        let md1 = "# Root\n\n## Child\n\nText.\n";
        tracker.update(md1, Some("alice"));
        assert_eq!(tracker.last_edited_by("root/child"), Some("alice"));

        let md2 = "# Root\n\nNo child.\n";
        tracker.update(md2, Some("bob"));
        assert_eq!(tracker.last_edited_by("root/child"), None);
        assert_eq!(tracker.last_edited_by("root"), Some("bob"));
    }

    #[test]
    fn tracker_no_attribution_without_editor_name() {
        let mut tracker = SectionTracker::new();
        let md = "# Root\n\nContent.\n";
        tracker.update(md, None);

        assert_eq!(tracker.last_edited_by("root"), None);
    }

    #[test]
    fn tracker_preserves_attribution_for_unchanged_sections() {
        let mut tracker = SectionTracker::new();

        let md1 = "# Root\n\n## A\n\nAlpha.\n\n## B\n\nBeta.\n";
        tracker.update(md1, Some("alice"));
        assert_eq!(tracker.last_edited_by("root/a"), Some("alice"));
        assert_eq!(tracker.last_edited_by("root/b"), Some("alice"));

        // Only modify section B.
        let md2 = "# Root\n\n## A\n\nAlpha.\n\n## B\n\nBeta updated.\n";
        tracker.update(md2, Some("bob"));

        // A's attribution should still be alice (content unchanged).
        assert_eq!(tracker.last_edited_by("root/a"), Some("alice"));
        // B was modified → now bob.
        assert_eq!(tracker.last_edited_by("root/b"), Some("bob"));
    }

    #[test]
    fn tracker_handles_deeply_nested_sections() {
        let mut tracker = SectionTracker::new();

        let md = "\
# Root

## API

### Authentication

OAuth2 PKCE flow.

### Authorization

Role-based access.

## Data

### Models

User, Document, Section.
";
        let diff = tracker.update(md, Some("claude"));

        assert_eq!(tracker.sections().len(), 6);
        assert_eq!(diff.added().count(), 6);

        // All sections attributed to claude.
        for section in tracker.sections() {
            assert_eq!(
                tracker.last_edited_by(&section.id),
                Some("claude"),
                "section {} should be attributed to claude",
                section.id
            );
        }

        // Verify ancestor chain IDs.
        let ids: Vec<&str> = tracker.sections().iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"root"));
        assert!(ids.contains(&"root/api"));
        assert!(ids.contains(&"root/api/authentication"));
        assert!(ids.contains(&"root/api/authorization"));
        assert!(ids.contains(&"root/data"));
        assert!(ids.contains(&"root/data/models"));
    }

    // ── SectionDiff convenience methods ─────────────────────────────

    #[test]
    fn diff_convenience_iterators_filter_correctly() {
        let diff = SectionDiff {
            changes: vec![
                SectionChange::Added(make_section("a", "Added")),
                SectionChange::Removed(make_section("b", "Removed")),
                SectionChange::Modified {
                    old: make_section("c", "Old"),
                    new: make_section("c", "New"),
                },
            ],
        };

        assert_eq!(diff.added().count(), 1);
        assert_eq!(diff.removed().count(), 1);
        assert_eq!(diff.modified().count(), 1);
        assert!(!diff.is_empty());
    }

    fn make_section(id: &str, heading: &str) -> Section {
        Section {
            id: id.to_string(),
            parent_id: None,
            heading: heading.to_string(),
            level: 1,
            start_line: 1,
            end_line: 2,
        }
    }
}
