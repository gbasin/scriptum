// Section overlap detection from editor cursor positions.
//
// Given a set of editor positions (cursor line + offset) and the current
// section tree, determines which editors share a section. Two or more
// editors in the same section produce a `SectionOverlap`. Severity is
// classified as:
//
//   - Info:    editors in the same section but different paragraphs
//   - Warning: editors in the same paragraph (contiguous non-blank lines)

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use scriptum_common::types::{EditorType, OverlapEditor, OverlapSeverity, Section, SectionOverlap};

/// An editor's current cursor position in a document.
#[derive(Debug, Clone)]
pub struct EditorPosition {
    pub name: String,
    pub editor_type: EditorType,
    /// 1-based line number in the document.
    pub line: u32,
    /// Character offset within the line.
    pub ch: u32,
    pub last_edit_at: DateTime<Utc>,
}

/// Detect section overlaps from editor positions and the section tree.
///
/// For each section containing 2+ editors, returns a `SectionOverlap`
/// with the appropriate severity. Results are sorted by section ID.
pub fn detect_overlaps(
    sections: &[Section],
    editors: &[EditorPosition],
    content: &str,
) -> Vec<SectionOverlap> {
    if editors.len() < 2 || sections.is_empty() {
        return Vec::new();
    }

    // Map each editor to its containing section.
    let mut editors_by_section: HashMap<&str, Vec<(&Section, &EditorPosition)>> = HashMap::new();

    for editor in editors {
        if let Some(section) = find_section_for_line(sections, editor.line) {
            editors_by_section.entry(section.id.as_str()).or_default().push((section, editor));
        }
    }

    // Build paragraph map for warning classification.
    let paragraph_map = build_paragraph_map(content);

    let mut overlaps: Vec<SectionOverlap> = editors_by_section
        .into_iter()
        .filter(|(_, group)| group.len() >= 2)
        .map(|(_, group)| {
            let section = group[0].0.clone();

            let overlap_editors: Vec<OverlapEditor> = group
                .iter()
                .map(|(sec, editor)| {
                    let cursor_offset = compute_cursor_offset(sec, editor.line, editor.ch);
                    OverlapEditor {
                        name: editor.name.clone(),
                        editor_type: editor.editor_type,
                        cursor_offset,
                        last_edit_at: editor.last_edit_at,
                    }
                })
                .collect();

            let severity = classify_severity(&group, &paragraph_map);

            SectionOverlap { section, editors: overlap_editors, severity }
        })
        .collect();

    overlaps.sort_by(|a, b| a.section.id.cmp(&b.section.id));
    overlaps
}

/// Find the most specific (deepest) section containing a given line.
///
/// Sections are matched by their [start_line, end_line) range. When
/// multiple sections contain the line (e.g., a heading and its parent),
/// the deepest (highest level number) is returned.
fn find_section_for_line(sections: &[Section], line: u32) -> Option<&Section> {
    sections.iter().filter(|s| line >= s.start_line && line < s.end_line).max_by_key(|s| s.level)
}

/// Compute cursor offset within a section (characters from section start).
fn compute_cursor_offset(section: &Section, line: u32, ch: u32) -> u32 {
    let lines_into_section = line.saturating_sub(section.start_line);
    // Approximate: each line adds ~80 chars (we use lines * 80 + ch).
    // For exact offset we'd need the content, but this is sufficient for
    // overlap detection and display.
    lines_into_section * 80 + ch
}

/// Build a map from 1-based line number to paragraph index.
///
/// A "paragraph" is a contiguous run of non-blank lines. Blank lines
/// get their own index (so two editors on different blank lines are not
/// in the same paragraph).
fn build_paragraph_map(content: &str) -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    let mut paragraph_idx: u32 = 0;
    let mut in_paragraph = false;

    for (i, line) in content.lines().enumerate() {
        let line_num = (i + 1) as u32;
        let is_blank = line.trim().is_empty();

        if is_blank {
            in_paragraph = false;
            // Blank lines get a unique paragraph index so they never match.
            paragraph_idx += 1;
            map.insert(line_num, paragraph_idx);
            paragraph_idx += 1;
        } else {
            if !in_paragraph {
                in_paragraph = true;
                paragraph_idx += 1;
            }
            map.insert(line_num, paragraph_idx);
        }
    }

    map
}

/// Classify overlap severity based on paragraph proximity.
///
/// If any two editors in the group share the same paragraph → Warning.
/// Otherwise → Info (same section but different paragraphs).
fn classify_severity(
    group: &[(&Section, &EditorPosition)],
    paragraph_map: &HashMap<u32, u32>,
) -> OverlapSeverity {
    let paragraph_ids: Vec<Option<&u32>> =
        group.iter().map(|(_, editor)| paragraph_map.get(&editor.line)).collect();

    for i in 0..paragraph_ids.len() {
        for j in (i + 1)..paragraph_ids.len() {
            if let (Some(a), Some(b)) = (paragraph_ids[i], paragraph_ids[j]) {
                if a == b {
                    return OverlapSeverity::Warning;
                }
            }
        }
    }

    OverlapSeverity::Info
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use scriptum_common::section::parser::parse_sections;
    use scriptum_common::types::{EditorType, OverlapSeverity};

    use super::*;

    fn editor(name: &str, editor_type: EditorType, line: u32, ch: u32) -> EditorPosition {
        EditorPosition { name: name.to_string(), editor_type, line, ch, last_edit_at: Utc::now() }
    }

    // ── detect_overlaps ─────────────────────────────────────────────

    #[test]
    fn no_overlap_with_fewer_than_two_editors() {
        let md = "# Root\n\nContent.\n";
        let sections = parse_sections(md);
        let editors = vec![editor("alice", EditorType::Human, 1, 0)];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert!(overlaps.is_empty());
    }

    #[test]
    fn no_overlap_with_empty_sections() {
        let editors =
            vec![editor("alice", EditorType::Human, 1, 0), editor("bob", EditorType::Human, 2, 0)];
        let overlaps = detect_overlaps(&[], &editors, "no headings");
        assert!(overlaps.is_empty());
    }

    #[test]
    fn no_overlap_when_editors_in_different_sections() {
        let md = "# Section A\n\nAlpha text.\n\n# Section B\n\nBeta text.\n";
        let sections = parse_sections(md);

        let editors = vec![
            editor("alice", EditorType::Human, 2, 0), // in Section A body
            editor("bob", EditorType::Human, 6, 0),   // in Section B body
        ];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert!(overlaps.is_empty());
    }

    #[test]
    fn overlap_detected_when_two_editors_in_same_section() {
        let md = "# Root\n\nFirst paragraph.\n\nSecond paragraph.\n";
        let sections = parse_sections(md);

        let editors =
            vec![editor("alice", EditorType::Human, 3, 0), editor("bob", EditorType::Agent, 5, 0)];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].section.heading, "Root");
        assert_eq!(overlaps[0].editors.len(), 2);
        // Different paragraphs → Info severity.
        assert_eq!(overlaps[0].severity, OverlapSeverity::Info);
    }

    #[test]
    fn warning_severity_when_editors_in_same_paragraph() {
        let md = "# Root\n\nLine one of paragraph.\nLine two of paragraph.\nLine three.\n";
        let sections = parse_sections(md);

        let editors =
            vec![editor("alice", EditorType::Human, 3, 5), editor("bob", EditorType::Human, 4, 10)];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn info_severity_when_editors_in_different_paragraphs_same_section() {
        let md = "# Root\n\nFirst paragraph text.\n\nSecond paragraph text.\n";
        let sections = parse_sections(md);

        let editors = vec![
            editor("alice", EditorType::Human, 3, 0), // first paragraph
            editor("bob", EditorType::Human, 5, 0),   // second paragraph
        ];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].severity, OverlapSeverity::Info);
    }

    #[test]
    fn multiple_overlaps_in_different_sections() {
        let md = "# A\n\nContent A.\n\n# B\n\nContent B.\n";
        let sections = parse_sections(md);

        let editors = vec![
            editor("alice", EditorType::Human, 3, 0),
            editor("bob", EditorType::Agent, 3, 5),
            editor("charlie", EditorType::Human, 7, 0),
            editor("dave", EditorType::Agent, 7, 3),
        ];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 2);
        // Sorted by section ID.
        assert_eq!(overlaps[0].section.heading, "A");
        assert_eq!(overlaps[0].editors.len(), 2);
        assert_eq!(overlaps[1].section.heading, "B");
        assert_eq!(overlaps[1].editors.len(), 2);
    }

    #[test]
    fn three_editors_in_one_section() {
        let md = "# Root\n\nBody text here.\n";
        let sections = parse_sections(md);

        let editors = vec![
            editor("alice", EditorType::Human, 3, 0),
            editor("bob", EditorType::Agent, 3, 5),
            editor("charlie", EditorType::Human, 3, 10),
        ];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].editors.len(), 3);
        // All on same line/paragraph → Warning.
        assert_eq!(overlaps[0].severity, OverlapSeverity::Warning);
    }

    #[test]
    fn nested_sections_use_deepest_match() {
        let md = "# Root\n\n## Child\n\nChild text.\n";
        let sections = parse_sections(md);

        // Line 5 is in Child body (which is nested under Root).
        let editors =
            vec![editor("alice", EditorType::Human, 5, 0), editor("bob", EditorType::Human, 5, 5)];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 1);
        // Should match Child (deeper), not Root.
        assert_eq!(overlaps[0].section.heading, "Child");
    }

    #[test]
    fn editor_type_preserved_in_overlap() {
        let md = "# Root\n\nContent.\n";
        let sections = parse_sections(md);

        let editors = vec![
            editor("human-user", EditorType::Human, 3, 0),
            editor("ai-agent", EditorType::Agent, 3, 5),
        ];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 1);

        let names: Vec<&str> = overlaps[0].editors.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"human-user"));
        assert!(names.contains(&"ai-agent"));

        let human = overlaps[0].editors.iter().find(|e| e.name == "human-user").unwrap();
        let agent = overlaps[0].editors.iter().find(|e| e.name == "ai-agent").unwrap();
        assert_eq!(human.editor_type, EditorType::Human);
        assert_eq!(agent.editor_type, EditorType::Agent);
    }

    #[test]
    fn cursor_offset_computed_from_section_start() {
        let md = "# Root\n\n## Child\n\nChild body line 1.\nChild body line 2.\n";
        let sections = parse_sections(md);

        // Child starts at line 3. Editor on line 5 = 2 lines into section.
        let editors =
            vec![editor("alice", EditorType::Human, 5, 10), editor("bob", EditorType::Human, 6, 0)];

        let overlaps = detect_overlaps(&sections, &editors, md);
        assert_eq!(overlaps.len(), 1);

        let alice = overlaps[0].editors.iter().find(|e| e.name == "alice").unwrap();
        let bob = overlaps[0].editors.iter().find(|e| e.name == "bob").unwrap();
        // alice: 2 lines into section (5 - 3) * 80 + 10 = 170
        assert_eq!(alice.cursor_offset, 170);
        // bob: 3 lines into section (6 - 3) * 80 + 0 = 240
        assert_eq!(bob.cursor_offset, 240);
    }

    // ── build_paragraph_map ─────────────────────────────────────────

    #[test]
    fn paragraph_map_groups_contiguous_lines() {
        let content = "Line one.\nLine two.\n\nLine four.\n";
        let map = build_paragraph_map(content);

        // Lines 1-2 are in the same paragraph.
        assert_eq!(map.get(&1), map.get(&2));
        // Line 3 (blank) is separate.
        assert_ne!(map.get(&2), map.get(&3));
        // Line 4 is a new paragraph.
        assert_ne!(map.get(&3), map.get(&4));
    }

    #[test]
    fn paragraph_map_empty_content() {
        let map = build_paragraph_map("");
        assert!(map.is_empty());
    }

    // ── find_section_for_line ───────────────────────────────────────

    #[test]
    fn find_section_returns_none_for_empty_sections() {
        assert!(find_section_for_line(&[], 1).is_none());
    }

    #[test]
    fn find_section_returns_deepest_match() {
        let md = "# Root\n\n## Child\n\nContent.\n";
        let sections = parse_sections(md);

        // Line 5 (Content.) is in both Root and Child.
        let found = find_section_for_line(&sections, 5);
        assert_eq!(found.unwrap().heading, "Child");
    }
}
