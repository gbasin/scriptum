// Section slug generation and stable section ID construction.
//
// Slugs: lowercase, strip non-alphanumeric, hyphenate spaces.
// Fallback: h{level}_{index} when heading text is empty.
// Section ID: slug(ancestor_chain) + ordinal suffix.

/// Convert a heading string into a URL-safe slug.
///
/// - Lowercases all characters
/// - Replaces non-ASCII-alphanumeric characters with hyphens
/// - Collapses consecutive hyphens
/// - Strips leading and trailing hyphens
///
/// Returns an empty string if the heading contains no alphanumeric characters.
pub fn slugify(heading: &str) -> String {
    let raw: String = heading
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();

    raw.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-")
}

/// Build a stable section ID from ancestor slugs and the section's own heading.
///
/// The ID encodes the full ancestor chain for structural stability:
/// `ancestor1/ancestor2/this-heading`. If two sibling sections produce the
/// same slug, an ordinal suffix (`~2`, `~3`, …) disambiguates.
///
/// # Arguments
/// - `ancestor_slugs` — slugs of all ancestor sections (root-first order)
/// - `heading` — the raw heading text for this section
/// - `level` — heading level (1–6), used for the empty-heading fallback
/// - `ordinal` — 1-based position among all sections in the document
pub fn make_section_id(
    ancestor_slugs: &[&str],
    heading: &str,
    level: u8,
    ordinal: usize,
) -> String {
    let slug = slugify(heading);
    let leaf = if slug.is_empty() { format!("h{}_{}", level, ordinal) } else { slug };

    if ancestor_slugs.is_empty() {
        leaf
    } else {
        format!("{}/{}", ancestor_slugs.join("/"), leaf)
    }
}

/// Append an ordinal suffix to a section ID to disambiguate duplicates.
///
/// - `base_id` — the section ID without suffix
/// - `occurrence` — which occurrence this is (1 = first, no suffix)
///
/// Returns `base_id` unchanged when `occurrence == 1`.
pub fn disambiguate(base_id: &str, occurrence: usize) -> String {
    if occurrence <= 1 {
        base_id.to_string()
    } else {
        format!("{base_id}~{occurrence}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── slugify ──────────────────────────────────────────────────────

    #[test]
    fn slugify_lowercases_and_hyphenates() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn slugify_strips_special_characters() {
        assert_eq!(slugify("API: Authentication (v2)"), "api-authentication-v2");
    }

    #[test]
    fn slugify_collapses_consecutive_hyphens() {
        assert_eq!(slugify("a---b"), "a-b");
        assert_eq!(slugify("  spaced  out  "), "spaced-out");
    }

    #[test]
    fn slugify_strips_leading_trailing_hyphens() {
        assert_eq!(slugify("--hello--"), "hello");
        assert_eq!(slugify("  -hello-  "), "hello");
    }

    #[test]
    fn slugify_returns_empty_for_no_alphanumeric() {
        assert_eq!(slugify("---"), "");
        assert_eq!(slugify("!@#$%"), "");
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("   "), "");
    }

    #[test]
    fn slugify_preserves_numbers() {
        assert_eq!(slugify("Phase 2: Setup"), "phase-2-setup");
    }

    #[test]
    fn slugify_handles_unicode_by_replacing_with_hyphens() {
        // Non-ASCII chars become hyphens and collapse
        assert_eq!(slugify("Über Cool"), "ber-cool");
        assert_eq!(slugify("日本語"), "");
    }

    #[test]
    fn slugify_handles_inline_code() {
        assert_eq!(slugify("The `parse` function"), "the-parse-function");
    }

    // ── make_section_id ──────────────────────────────────────────────

    #[test]
    fn section_id_without_ancestors() {
        let id = make_section_id(&[], "Authentication", 2, 1);
        assert_eq!(id, "authentication");
    }

    #[test]
    fn section_id_with_ancestors() {
        let id = make_section_id(&["root", "api"], "Authentication", 3, 5);
        assert_eq!(id, "root/api/authentication");
    }

    #[test]
    fn section_id_fallback_for_empty_heading() {
        let id = make_section_id(&["root"], "", 2, 3);
        assert_eq!(id, "root/h2_3");
    }

    #[test]
    fn section_id_fallback_without_ancestors() {
        let id = make_section_id(&[], "", 1, 1);
        assert_eq!(id, "h1_1");
    }

    #[test]
    fn section_id_fallback_for_non_alphanumeric_heading() {
        let id = make_section_id(&["top"], "---", 3, 7);
        assert_eq!(id, "top/h3_7");
    }

    // ── disambiguate ─────────────────────────────────────────────────

    #[test]
    fn disambiguate_first_occurrence_unchanged() {
        assert_eq!(disambiguate("root/auth", 1), "root/auth");
    }

    #[test]
    fn disambiguate_second_occurrence_gets_suffix() {
        assert_eq!(disambiguate("root/auth", 2), "root/auth~2");
    }

    #[test]
    fn disambiguate_high_ordinal() {
        assert_eq!(disambiguate("section", 42), "section~42");
    }

    // ── integration: ancestor chain → section_id ─────────────────────

    #[test]
    fn ancestor_chain_builds_expected_ids() {
        // Simulating:
        // # Root
        // ## API
        // ### Authentication
        // ### Authorization
        // ## Data

        let root_id = make_section_id(&[], "Root", 1, 1);
        assert_eq!(root_id, "root");

        let root_slug = slugify("Root");
        let api_id = make_section_id(&[&root_slug], "API", 2, 2);
        assert_eq!(api_id, "root/api");

        let api_slug = slugify("API");
        let auth_id = make_section_id(&[&root_slug, &api_slug], "Authentication", 3, 3);
        assert_eq!(auth_id, "root/api/authentication");

        let authz_id = make_section_id(&[&root_slug, &api_slug], "Authorization", 3, 4);
        assert_eq!(authz_id, "root/api/authorization");

        let data_id = make_section_id(&[&root_slug], "Data", 2, 5);
        assert_eq!(data_id, "root/data");
    }

    #[test]
    fn duplicate_sibling_headings_are_disambiguated() {
        let base1 = make_section_id(&["doc"], "Overview", 2, 1);
        let base2 = make_section_id(&["doc"], "Overview", 2, 3);

        // Same slug — needs disambiguation
        assert_eq!(base1, base2);

        let id1 = disambiguate(&base1, 1);
        let id2 = disambiguate(&base2, 2);
        assert_eq!(id1, "doc/overview");
        assert_eq!(id2, "doc/overview~2");
        assert_ne!(id1, id2);
    }
}
