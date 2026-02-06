use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::types::Section;

#[derive(Debug, Clone)]
struct SectionDraft {
    heading: String,
    level: u8,
    start_line: u32,
}

pub fn parse_sections(markdown: &str) -> Vec<Section> {
    let mut drafts = Vec::new();
    let mut current_heading: Option<SectionDraft> = None;

    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if !is_atx_heading(markdown, range.start) {
                    current_heading = None;
                    continue;
                }

                current_heading = Some(SectionDraft {
                    heading: String::new(),
                    level: level_to_u8(level),
                    start_line: line_number_for_offset(markdown, range.start),
                });
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = current_heading.as_mut() {
                    heading.heading.push_str(&text);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = current_heading.take() {
                    drafts.push(heading);
                }
            }
            _ => {}
        }
    }

    build_tree(markdown, drafts)
}

fn build_tree(markdown: &str, drafts: Vec<SectionDraft>) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::with_capacity(drafts.len());
    let mut stack: Vec<usize> = Vec::new();
    let total_lines = markdown.lines().count() as u32;

    for (index, draft) in drafts.iter().enumerate() {
        while let Some(last_index) = stack.last().copied() {
            if sections[last_index].level >= draft.level {
                stack.pop();
            } else {
                break;
            }
        }

        let parent_id = stack.last().map(|parent_index| sections[*parent_index].id.clone());
        let id = make_section_id(draft.level, &draft.heading, index + 1);
        let end_line = drafts.get(index + 1).map(|next| next.start_line).unwrap_or(total_lines + 1);

        sections.push(Section {
            id,
            parent_id,
            heading: draft.heading.trim().to_string(),
            level: draft.level,
            start_line: draft.start_line,
            end_line,
        });
        stack.push(index);
    }

    sections
}

fn make_section_id(level: u8, heading: &str, ordinal: usize) -> String {
    let slug = heading
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        format!("h{level}:{ordinal}")
    } else {
        format!("h{level}:{slug}")
    }
}

fn is_atx_heading(markdown: &str, offset: usize) -> bool {
    let line_start = markdown[..offset].rfind('\n').map(|index| index + 1).unwrap_or(0);
    markdown[line_start..]
        .chars()
        .find(|ch| !ch.is_whitespace())
        .map(|ch| ch == '#')
        .unwrap_or(false)
}

fn line_number_for_offset(markdown: &str, offset: usize) -> u32 {
    markdown[..offset].bytes().filter(|byte| *byte == b'\n').count() as u32 + 1
}

fn level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_sections;

    #[test]
    fn parses_only_atx_headings_and_builds_parent_links() {
        let markdown = "# Root\n\n## Child\n\n### Grandchild\n\n## Sibling\n";
        let sections = parse_sections(markdown);

        assert_eq!(sections.len(), 4);

        assert_eq!(sections[0].heading, "Root");
        assert_eq!(sections[0].parent_id, None);
        assert_eq!(sections[0].start_line, 1);
        assert_eq!(sections[0].end_line, 3);

        assert_eq!(sections[1].heading, "Child");
        assert_eq!(sections[1].parent_id.as_deref(), Some("h1:root"));
        assert_eq!(sections[1].start_line, 3);
        assert_eq!(sections[1].end_line, 5);

        assert_eq!(sections[2].heading, "Grandchild");
        assert_eq!(sections[2].parent_id.as_deref(), Some("h2:child"));
        assert_eq!(sections[2].start_line, 5);
        assert_eq!(sections[2].end_line, 7);

        assert_eq!(sections[3].heading, "Sibling");
        assert_eq!(sections[3].parent_id.as_deref(), Some("h1:root"));
        assert_eq!(sections[3].start_line, 7);
    }

    #[test]
    fn ignores_setext_headings() {
        let markdown = "Title\n=====\n\n# Actual\n";
        let sections = parse_sections(markdown);

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, "Actual");
    }

    #[test]
    fn ignores_hash_lines_in_code_blocks_and_html_blocks() {
        let markdown = r#"# Real

```
# Not a section
```

<div>
# Also not a section
</div>

## Next
"#;

        let sections = parse_sections(markdown);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "Real");
        assert_eq!(sections[1].heading, "Next");
    }
}
