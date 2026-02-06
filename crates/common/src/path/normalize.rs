// Path canonicalization: NFKC normalization, traversal rejection, 512 char max.

use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// Maximum allowed path length in characters.
const MAX_PATH_CHARS: usize = 512;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PathError {
    #[error("path is empty")]
    Empty,

    #[error("path exceeds maximum length of {MAX_PATH_CHARS} characters")]
    TooLong,

    #[error("path contains directory traversal component: {0}")]
    Traversal(String),

    #[error("path contains null byte")]
    NullByte,

    #[error("path contains invalid component: {0}")]
    InvalidComponent(String),
}

/// Normalize a document path for safe storage and uniqueness checking.
///
/// Rules:
/// - Apply Unicode NFKC normalization
/// - Convert all separators to `/`
/// - Collapse consecutive `/` into one
/// - Strip leading and trailing `/`
/// - Reject `.` and `..` path components (traversal)
/// - Reject null bytes
/// - Reject empty paths
/// - Enforce max 512 character limit (after normalization)
pub fn normalize_path(input: &str) -> Result<String, PathError> {
    if input.is_empty() {
        return Err(PathError::Empty);
    }

    if input.contains('\0') {
        return Err(PathError::NullByte);
    }

    // Apply Unicode NFKC normalization
    let normalized: String = input.nfkc().collect();

    // Convert backslashes to forward slashes
    let unified = normalized.replace('\\', "/");

    // Split into components, filter empty segments (from consecutive slashes)
    let components: Vec<&str> = unified.split('/').filter(|s| !s.is_empty()).collect();

    if components.is_empty() {
        return Err(PathError::Empty);
    }

    // Validate each component
    for component in &components {
        if *component == "." {
            return Err(PathError::Traversal(".".to_string()));
        }
        if *component == ".." {
            return Err(PathError::Traversal("..".to_string()));
        }
        // Reject components that are only whitespace
        if component.trim().is_empty() {
            return Err(PathError::InvalidComponent(
                "(whitespace-only component)".to_string(),
            ));
        }
    }

    let result = components.join("/");

    if result.chars().count() > MAX_PATH_CHARS {
        return Err(PathError::TooLong);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Valid paths
    #[test]
    fn test_simple_path() {
        assert_eq!(normalize_path("docs/readme.md").unwrap(), "docs/readme.md");
    }

    #[test]
    fn test_backslash_to_forward() {
        assert_eq!(
            normalize_path("docs\\notes\\file.md").unwrap(),
            "docs/notes/file.md"
        );
    }

    #[test]
    fn test_strip_leading_trailing_slash() {
        assert_eq!(normalize_path("/docs/file.md/").unwrap(), "docs/file.md");
    }

    #[test]
    fn test_collapse_consecutive_slashes() {
        assert_eq!(
            normalize_path("docs///nested//file.md").unwrap(),
            "docs/nested/file.md"
        );
    }

    #[test]
    fn test_single_filename() {
        assert_eq!(normalize_path("readme.md").unwrap(), "readme.md");
    }

    #[test]
    fn test_unicode_nfkc() {
        // NFKC normalizes ﬁ (U+FB01, fi ligature) to "fi"
        assert_eq!(normalize_path("docs/\u{FB01}le.md").unwrap(), "docs/file.md");
    }

    #[test]
    fn test_unicode_combining() {
        // NFKC normalizes é (e + combining accent) to a single char
        let composed = normalize_path("docs/caf\u{0065}\u{0301}.md").unwrap();
        let expected = normalize_path("docs/café.md").unwrap();
        assert_eq!(composed, expected);
    }

    // Traversal attacks
    #[test]
    fn test_reject_dotdot() {
        assert_eq!(
            normalize_path("docs/../etc/passwd"),
            Err(PathError::Traversal("..".to_string()))
        );
    }

    #[test]
    fn test_reject_leading_dotdot() {
        assert_eq!(
            normalize_path("../../../etc/passwd"),
            Err(PathError::Traversal("..".to_string()))
        );
    }

    #[test]
    fn test_reject_dot_component() {
        assert_eq!(
            normalize_path("docs/./file.md"),
            Err(PathError::Traversal(".".to_string()))
        );
    }

    #[test]
    fn test_reject_backslash_traversal() {
        assert_eq!(
            normalize_path("docs\\..\\etc\\passwd"),
            Err(PathError::Traversal("..".to_string()))
        );
    }

    // Edge cases
    #[test]
    fn test_reject_empty() {
        assert_eq!(normalize_path(""), Err(PathError::Empty));
    }

    #[test]
    fn test_reject_only_slashes() {
        assert_eq!(normalize_path("///"), Err(PathError::Empty));
    }

    #[test]
    fn test_reject_null_byte() {
        assert_eq!(normalize_path("docs/file\0.md"), Err(PathError::NullByte));
    }

    #[test]
    fn test_reject_too_long() {
        let long_path = "a/".repeat(300);
        assert_eq!(normalize_path(&long_path), Err(PathError::TooLong));
    }

    #[test]
    fn test_max_length_exactly() {
        // 512 chars is allowed
        let path = "a".repeat(512);
        assert!(normalize_path(&path).is_ok());
    }

    #[test]
    fn test_over_max_length() {
        let path = "a".repeat(513);
        assert_eq!(normalize_path(&path), Err(PathError::TooLong));
    }

    // Filenames that look dangerous but are valid
    #[test]
    fn test_dotfile_allowed() {
        assert_eq!(normalize_path(".gitignore").unwrap(), ".gitignore");
    }

    #[test]
    fn test_hidden_dir_allowed() {
        assert_eq!(
            normalize_path(".config/settings.md").unwrap(),
            ".config/settings.md"
        );
    }

    #[test]
    fn test_dots_in_filename_allowed() {
        assert_eq!(
            normalize_path("file.backup.2024.md").unwrap(),
            "file.backup.2024.md"
        );
    }

    #[test]
    fn test_triple_dot_filename_allowed() {
        // "..." as a filename is valid (not . or ..)
        assert_eq!(normalize_path("docs/...").unwrap(), "docs/...");
    }
}
