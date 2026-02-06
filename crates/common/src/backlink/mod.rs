// Wiki-style backlink parsing (`[[target]]` syntax).
//
// Supported forms:
// - [[target]]
// - [[target|alias]]
// - [[target#heading]]
// - [[target#heading|alias]] (Obsidian-compatible superset)

/// A parsed wiki-style link from markdown content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    /// Target document text before any `#heading` fragment.
    pub target: String,
    /// Optional section fragment after `#`.
    pub heading: Option<String>,
    /// Optional display alias after `|`.
    pub alias: Option<String>,
    /// Raw inner text between `[[` and `]]`.
    pub raw: String,
    /// Byte offset of the opening `[[`.
    pub start_offset: usize,
    /// Byte offset just after the closing `]]`.
    pub end_offset: usize,
}

/// Parse Obsidian-style wiki links from markdown.
///
/// This parser is intentionally lightweight and works on plain markdown text,
/// making it suitable for "extract on save/commit" indexing workflows.
pub fn parse_wiki_links(markdown: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();
    let bytes = markdown.as_bytes();
    let mut index = 0usize;

    while index + 1 < bytes.len() {
        if bytes[index] == b'[' && bytes[index + 1] == b'[' {
            let start = index;
            index += 2;

            let mut close = None;
            while index + 1 < bytes.len() {
                if bytes[index] == b']' && bytes[index + 1] == b']' {
                    close = Some(index);
                    break;
                }
                index += 1;
            }

            let Some(close_start) = close else {
                break;
            };

            let inner = &markdown[start + 2..close_start];
            if let Some(link) = parse_inner_link(inner, start, close_start + 2) {
                links.push(link);
            }
            index = close_start + 2;
            continue;
        }

        index += 1;
    }

    links
}

fn parse_inner_link(inner: &str, start_offset: usize, end_offset: usize) -> Option<WikiLink> {
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (target_part, alias_part) = match trimmed.split_once('|') {
        Some((left, right)) => (left.trim(), Some(right.trim())),
        None => (trimmed, None),
    };

    if target_part.is_empty() {
        return None;
    }

    let (target_raw, heading_raw) = match target_part.split_once('#') {
        Some((target, heading)) => (target.trim(), Some(heading.trim())),
        None => (target_part, None),
    };

    if target_raw.is_empty() {
        return None;
    }

    let heading = heading_raw.and_then(|value| (!value.is_empty()).then(|| value.to_string()));
    let alias = alias_part.and_then(|value| (!value.is_empty()).then(|| value.to_string()));

    Some(WikiLink {
        target: target_raw.to_string(),
        heading,
        alias,
        raw: trimmed.to_string(),
        start_offset,
        end_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_wiki_links;

    #[test]
    fn parses_basic_target_link() {
        let links = parse_wiki_links("See [[Auth]].");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Auth");
        assert_eq!(links[0].heading, None);
        assert_eq!(links[0].alias, None);
        assert_eq!(links[0].raw, "Auth");
    }

    #[test]
    fn parses_target_with_alias() {
        let links = parse_wiki_links("See [[Auth|Authentication Flow]].");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Auth");
        assert_eq!(links[0].heading, None);
        assert_eq!(links[0].alias.as_deref(), Some("Authentication Flow"));
    }

    #[test]
    fn parses_target_with_heading_fragment() {
        let links = parse_wiki_links("See [[Auth#PKCE]].");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Auth");
        assert_eq!(links[0].heading.as_deref(), Some("PKCE"));
        assert_eq!(links[0].alias, None);
    }

    #[test]
    fn parses_heading_and_alias_together() {
        let links = parse_wiki_links("See [[Auth#PKCE|OAuth PKCE]].");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Auth");
        assert_eq!(links[0].heading.as_deref(), Some("PKCE"));
        assert_eq!(links[0].alias.as_deref(), Some("OAuth PKCE"));
    }

    #[test]
    fn parses_multiple_links_in_document() {
        let links = parse_wiki_links("[[One]] and [[Two|Second]] and [[Three#H3]].");

        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "One");
        assert_eq!(links[1].target, "Two");
        assert_eq!(links[2].target, "Three");
        assert_eq!(links[1].alias.as_deref(), Some("Second"));
        assert_eq!(links[2].heading.as_deref(), Some("H3"));
    }

    #[test]
    fn trims_whitespace_inside_link() {
        let links = parse_wiki_links("[[  docs/api  #  auth  |  API Auth  ]]");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "docs/api");
        assert_eq!(links[0].heading.as_deref(), Some("auth"));
        assert_eq!(links[0].alias.as_deref(), Some("API Auth"));
    }

    #[test]
    fn ignores_empty_or_malformed_links() {
        let links = parse_wiki_links("[[]] [[|alias]] [[#heading]] [[open");
        assert!(links.is_empty());
    }

    #[test]
    fn preserves_source_offsets() {
        let markdown = "A [[One]] B [[Two|2]]";
        let links = parse_wiki_links(markdown);

        assert_eq!((links[0].start_offset, links[0].end_offset), (2, 9));
        assert_eq!((links[1].start_offset, links[1].end_offset), (12, 21));
        assert_eq!(&markdown[links[0].start_offset..links[0].end_offset], "[[One]]");
        assert_eq!(&markdown[links[1].start_offset..links[1].end_offset], "[[Two|2]]");
    }
}
