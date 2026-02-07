use proptest::collection::vec;
use proptest::prelude::*;
use scriptum_common::diff::patch::apply_text_diff_to_ytext;
use yrs::{Doc, GetString, Text, Transact};

fn interesting_char() -> impl Strategy<Value = char> {
    prop_oneof![
        (b'a'..=b'z').prop_map(char::from),
        (b'A'..=b'Z').prop_map(char::from),
        (b'0'..=b'9').prop_map(char::from),
        Just(' '),
        Just('\n'),
        Just('\t'),
        Just('-'),
        Just('_'),
        Just('#'),
        Just('*'),
        Just('.'),
        Just(','),
        Just(':'),
        Just('🙂'),
        Just('🚀'),
        Just('中'),
        Just('文'),
        Just('界'),
        Just('あ'),
        Just('い'),
        Just('ا'),
        Just('ل'),
        Just('م'),
        Just('ש'),
        Just('ת'),
    ]
}

fn markdown_string(min_len: usize, max_len: usize) -> impl Strategy<Value = String> {
    vec(interesting_char(), min_len..max_len).prop_map(|chars| chars.into_iter().collect())
}

fn whitespace_string(max_len: usize) -> impl Strategy<Value = String> {
    vec(prop_oneof![Just(' '), Just('\n'), Just('\t'), Just('\r')], 0..max_len)
        .prop_map(|chars| chars.into_iter().collect())
}

fn assert_diff_roundtrip(old_text: &str, new_text: &str) {
    let doc = Doc::new();
    let ytext = doc.get_or_insert_text("content");
    {
        let mut txn = doc.transact_mut();
        ytext.insert(&mut txn, 0, old_text);
    }

    apply_text_diff_to_ytext(&doc, &ytext, old_text, new_text);
    let actual = ytext.get_string(&doc.transact());

    assert_eq!(
        actual,
        new_text,
        "diff roundtrip mismatch: old_len={} new_len={}",
        old_text.len(),
        new_text.len()
    );
}

fn build_large_markdown(paragraphs: usize, marker: &str) -> String {
    let mut out = String::with_capacity(130_000);
    for i in 0..paragraphs {
        out.push_str("## Section ");
        out.push_str(&i.to_string());
        out.push('\n');
        out.push_str("Marker: ");
        out.push_str(marker);
        out.push('\n');
        out.push_str("Body line A with repeated content for diff stress.\n");
        out.push_str("Body line B with tabs\tand spaces.\n\n");
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        max_shrink_iters: 128,
        .. ProptestConfig::default()
    })]

    #[test]
    fn diff_to_yjs_matches_random_before_after_strings(
        before in markdown_string(0, 320),
        after in markdown_string(0, 320),
    ) {
        assert_diff_roundtrip(&before, &after);
    }

    #[test]
    fn diff_to_yjs_handles_whitespace_only_changes(
        before in whitespace_string(500),
        after in whitespace_string(500),
    ) {
        assert_diff_roundtrip(&before, &after);
    }
}

#[test]
fn diff_to_yjs_handles_empty_and_non_empty_boundaries() {
    assert_diff_roundtrip("", "");
    assert_diff_roundtrip("", "hello");
    assert_diff_roundtrip("hello", "");
}

#[test]
fn diff_to_yjs_handles_single_character_changes() {
    let cases =
        [("abc", "axc"), ("abc", "abxc"), ("abc", "ac"), ("中a文", "中🙂文"), ("שלום", "שלו")];

    for (before, after) in cases {
        assert_diff_roundtrip(before, after);
    }
}

#[test]
fn diff_to_yjs_handles_large_documents_over_100kb() {
    let before = build_large_markdown(2_000, "🙂 中 文");
    let mut after = before.clone();

    // Rewrite multiple distant regions and append new sections to stress patch generation
    // on large documents without turning the whole file into an unrelated replacement.
    for section in [7, 83, 240, 512, 999, 1_501] {
        let old_block = format!(
            "## Section {section}\nMarker: 🙂 中 文\nBody line A with repeated content for diff stress.\nBody line B with tabs\tand spaces.\n\n"
        );
        let new_block = format!(
            "## Section {section}\nMarker: مرحبا שלום 🚀\nBody line A rewritten for large-doc patch validation.\nBody line B with tabs\tand spaces plus trailing markers ###.\n\n"
        );
        after = after.replacen(&old_block, &new_block, 1);
    }

    for i in 0..60 {
        after.push_str("### Added Tail Section ");
        after.push_str(&i.to_string());
        after.push('\n');
        after.push_str("Extra paragraph with unicode: こんにちは | שלום | مرحبا | 🚀\n\n");
    }

    assert!(before.len() > 100_000, "before document should exceed 100KB");
    assert!(after.len() > 100_000, "after document should exceed 100KB");

    assert_diff_roundtrip(&before, &after);
}
