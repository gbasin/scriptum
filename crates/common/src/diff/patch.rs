use yrs::{Doc, Text, TextRef, Transact};

pub const FILE_WATCHER_ORIGIN: &str = "file-watcher";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPatchOp {
    Insert { index: u32, text: String },
    Delete { index: u32, len: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharEdit {
    Equal(char),
    Insert(char),
    Delete(char),
}

/// Computes a patch-style diff from `old_text` to `new_text`.
///
/// Operations use UTF-8 byte offsets to match `yrs::Text` indexing.
pub fn diff_to_patch_ops(old_text: &str, new_text: &str) -> Vec<TextPatchOp> {
    if old_text == new_text {
        return Vec::new();
    }

    let old_chars: Vec<char> = old_text.chars().collect();
    let new_chars: Vec<char> = new_text.chars().collect();
    let edits = myers_char_edits(&old_chars, &new_chars);
    edits_to_patch_ops(&edits)
}

/// Applies precomputed patch operations to a Yjs text value.
///
/// Operations are executed within a transaction tagged by `origin`.
pub fn apply_patch_ops_to_ytext(
    doc: &Doc,
    ytext: &TextRef,
    patch_ops: &[TextPatchOp],
    origin: &str,
) {
    if patch_ops.is_empty() {
        return;
    }

    let mut txn = doc.transact_mut_with(origin);
    let mut offset: i64 = 0;

    for patch_op in patch_ops {
        match patch_op {
            TextPatchOp::Delete { index, len } => {
                let target = shifted_index(*index, offset);
                ytext.remove_range(&mut txn, target, *len);
                offset -= i64::from(*len);
            }
            TextPatchOp::Insert { index, text } => {
                let target = shifted_index(*index, offset);
                ytext.insert(&mut txn, target, text);
                offset += i64::from(utf8_len(text));
            }
        }
    }
}

/// Computes and applies patch operations in a single `file-watcher` transaction.
pub fn apply_text_diff_to_ytext(
    doc: &Doc,
    ytext: &TextRef,
    old_text: &str,
    new_text: &str,
) -> Vec<TextPatchOp> {
    let patch_ops = diff_to_patch_ops(old_text, new_text);
    apply_patch_ops_to_ytext(doc, ytext, &patch_ops, FILE_WATCHER_ORIGIN);
    patch_ops
}

fn shifted_index(index: u32, offset: i64) -> u32 {
    u32::try_from(i64::from(index) + offset)
        .expect("patch operation produced negative index after offset adjustment")
}

fn utf8_len(value: &str) -> u32 {
    value.len() as u32
}

fn myers_char_edits(old_chars: &[char], new_chars: &[char]) -> Vec<CharEdit> {
    let old_len = old_chars.len();
    let new_len = new_chars.len();

    if old_len == 0 {
        return new_chars.iter().copied().map(CharEdit::Insert).collect();
    }
    if new_len == 0 {
        return old_chars.iter().copied().map(CharEdit::Delete).collect();
    }

    let max = old_len + new_len;
    let offset = max as isize;
    let mut v = vec![0isize; 2 * max + 1];
    let mut trace: Vec<Vec<isize>> = Vec::with_capacity(max + 1);
    let mut solved_d = 0usize;

    'outer: for d in 0..=max {
        trace.push(v.clone());

        let d_isize = d as isize;
        let mut k = -d_isize;
        while k <= d_isize {
            let k_idx = (k + offset) as usize;
            let mut x = if k == -d_isize
                || (k != d_isize && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize])
            {
                v[(k + 1 + offset) as usize]
            } else {
                v[(k - 1 + offset) as usize] + 1
            };
            let mut y = x - k;

            while x < old_len as isize
                && y < new_len as isize
                && old_chars[x as usize] == new_chars[y as usize]
            {
                x += 1;
                y += 1;
            }

            v[k_idx] = x;

            if x >= old_len as isize && y >= new_len as isize {
                solved_d = d;
                break 'outer;
            }

            k += 2;
        }
    }

    backtrack_char_edits(old_chars, new_chars, &trace, solved_d, offset)
}

fn backtrack_char_edits(
    old_chars: &[char],
    new_chars: &[char],
    trace: &[Vec<isize>],
    solved_d: usize,
    offset: isize,
) -> Vec<CharEdit> {
    let mut edits = Vec::new();
    let mut x = old_chars.len() as isize;
    let mut y = new_chars.len() as isize;

    for d in (0..=solved_d).rev() {
        let v = &trace[d];
        let k = x - y;
        let d_isize = d as isize;

        let prev_k = if d == 0 {
            0
        } else if k == -d_isize
            || (k != d_isize && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize])
        {
            k + 1
        } else {
            k - 1
        };
        let prev_x = if d == 0 { 0 } else { v[(prev_k + offset) as usize] };
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            edits.push(CharEdit::Equal(old_chars[(x - 1) as usize]));
            x -= 1;
            y -= 1;
        }

        if d == 0 {
            break;
        }

        if x == prev_x {
            edits.push(CharEdit::Insert(new_chars[(y - 1) as usize]));
            y -= 1;
        } else {
            edits.push(CharEdit::Delete(old_chars[(x - 1) as usize]));
            x -= 1;
        }
    }

    edits.reverse();
    edits
}

fn edits_to_patch_ops(edits: &[CharEdit]) -> Vec<TextPatchOp> {
    let mut patch_ops = Vec::new();
    let mut old_index_bytes = 0u32;

    for edit in edits {
        match edit {
            CharEdit::Equal(ch) => {
                old_index_bytes += ch.len_utf8() as u32;
            }
            CharEdit::Delete(ch) => {
                let char_len = ch.len_utf8() as u32;
                match patch_ops.last_mut() {
                    Some(TextPatchOp::Delete { index, len })
                        if *index + *len == old_index_bytes =>
                    {
                        *len += char_len;
                    }
                    _ => {
                        patch_ops
                            .push(TextPatchOp::Delete { index: old_index_bytes, len: char_len });
                    }
                }
                old_index_bytes += char_len;
            }
            CharEdit::Insert(ch) => match patch_ops.last_mut() {
                Some(TextPatchOp::Insert { index, text }) if *index == old_index_bytes => {
                    text.push(*ch);
                }
                _ => {
                    patch_ops
                        .push(TextPatchOp::Insert { index: old_index_bytes, text: ch.to_string() });
                }
            },
        }
    }

    patch_ops
}

#[cfg(test)]
mod tests {
    use super::{apply_text_diff_to_ytext, diff_to_patch_ops, TextPatchOp, FILE_WATCHER_ORIGIN};
    use std::sync::{Arc, Mutex};
    use yrs::{Doc, GetString, Text, Transact};

    struct Lcg {
        state: u64,
    }

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            self.state
        }

        fn next_usize(&mut self, upper_exclusive: usize) -> usize {
            if upper_exclusive == 0 {
                return 0;
            }
            (self.next_u64() as usize) % upper_exclusive
        }

        fn next_char(&mut self) -> char {
            // Mix plain ASCII, whitespace, punctuation, and a few multibyte glyphs.
            match self.next_usize(52) {
                0..=25 => char::from(b'a' + self.next_usize(26) as u8),
                26..=35 => char::from(b'0' + self.next_usize(10) as u8),
                36 => ' ',
                37 => '\n',
                38 => '-',
                39 => '_',
                40 => '#',
                41 => '/',
                42 => '.',
                43 => ',',
                44 => ':',
                45 => ';',
                46 => '🙂',
                47 => '☕',
                48 => 'é',
                49 => 'ß',
                50 => '中',
                _ => '文',
            }
        }
    }

    fn random_string(rng: &mut Lcg, min_len: usize, max_len: usize) -> String {
        let span = max_len.saturating_sub(min_len).saturating_add(1);
        let len = min_len + rng.next_usize(span);
        let mut out = String::with_capacity(len);
        for _ in 0..len {
            out.push(rng.next_char());
        }
        out
    }

    fn mutate_text(rng: &mut Lcg, current: &str) -> String {
        let mut chars: Vec<char> = current.chars().collect();

        match rng.next_usize(5) {
            0 => {
                // Insert chunk
                let index = rng.next_usize(chars.len().saturating_add(1));
                let insert = random_string(rng, 1, 24);
                chars.splice(index..index, insert.chars());
            }
            1 if !chars.is_empty() => {
                // Delete range
                let start = rng.next_usize(chars.len());
                let max_len = chars.len() - start;
                let len = 1 + rng.next_usize(max_len);
                chars.drain(start..start + len);
            }
            2 if !chars.is_empty() => {
                // Replace range
                let start = rng.next_usize(chars.len());
                let max_len = chars.len() - start;
                let len = 1 + rng.next_usize(max_len);
                let replacement = random_string(rng, 0, 20);
                chars.splice(start..start + len, replacement.chars());
            }
            3 => {
                // Prefix/suffix burst
                if rng.next_usize(2) == 0 {
                    let prefix = random_string(rng, 0, 18);
                    chars.splice(0..0, prefix.chars());
                } else {
                    chars.extend(random_string(rng, 0, 18).chars());
                }
            }
            _ => {
                // Full rewrite occasionally (rapid-save + large-change pressure)
                return random_string(rng, 0, 200);
            }
        }

        chars.into_iter().collect()
    }

    #[test]
    fn computes_expected_simple_insert_and_delete_ops() {
        assert_eq!(
            diff_to_patch_ops("abc", "abXYZc"),
            vec![TextPatchOp::Insert { index: 2, text: "XYZ".to_owned() }]
        );

        assert_eq!(
            diff_to_patch_ops("abXYZc", "abc"),
            vec![TextPatchOp::Delete { index: 2, len: 3 }]
        );
    }

    #[test]
    fn uses_utf8_offsets_for_emoji_edits() {
        assert_eq!(
            diff_to_patch_ops("🙂a", "🙂🙂a"),
            vec![TextPatchOp::Insert { index: 4, text: "🙂".to_owned() }]
        );
    }

    #[test]
    fn applies_patch_ops_for_various_diff_scenarios() {
        let scenarios = [
            ("", "hello world"),
            ("hello world", ""),
            ("hello world", "hello brave new world"),
            ("alpha\nbeta\ngamma\n", "alpha!\nbeta\ndelta\ngamma\nomega\n"),
            ("naïve café", "naive cafe ☕"),
            ("🙂 hello", "🙂 hi"),
        ];

        for (old_text, new_text) in scenarios {
            let doc = Doc::new();
            let ytext = doc.get_or_insert_text("content");
            {
                let mut txn = doc.transact_mut();
                ytext.insert(&mut txn, 0, old_text);
            }

            let patch_ops = apply_text_diff_to_ytext(&doc, &ytext, old_text, new_text);
            let actual = ytext.get_string(&doc.transact());

            assert_eq!(actual, new_text, "failed scenario old={old_text:?} new={new_text:?}");
            if old_text == new_text {
                assert!(patch_ops.is_empty());
            }
        }
    }

    #[test]
    fn tags_transactions_with_file_watcher_origin() {
        let doc = Doc::new();
        let ytext = doc.get_or_insert_text("content");
        {
            let mut txn = doc.transact_mut();
            ytext.insert(&mut txn, 0, "old");
        }

        let captured_origin: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
        let captured_origin_for_cb = Arc::clone(&captured_origin);
        let _subscription = doc
            .observe_update_v1(move |txn, _| {
                let origin = txn.origin().map(|value| value.as_ref().to_vec());
                *captured_origin_for_cb.lock().expect("origin lock should be available") = origin;
            })
            .expect("subscription should register");

        apply_text_diff_to_ytext(&doc, &ytext, "old", "new");

        let origin = captured_origin
            .lock()
            .expect("origin lock should be available")
            .clone()
            .expect("origin should be captured");
        assert_eq!(origin, FILE_WATCHER_ORIGIN.as_bytes());
    }

    #[test]
    fn randomized_rapid_save_sequences_match_expected_content() {
        for seed in [11_u64, 42, 2_026, 65_537] {
            let doc = Doc::new();
            let ytext = doc.get_or_insert_text("content");
            let mut rng = Lcg::new(seed);
            let mut expected = random_string(&mut rng, 0, 120);

            {
                let mut txn = doc.transact_mut();
                ytext.insert(&mut txn, 0, &expected);
            }

            for _ in 0..250 {
                let old = expected.clone();
                let new = mutate_text(&mut rng, &old);
                apply_text_diff_to_ytext(&doc, &ytext, &old, &new);
                let actual = ytext.get_string(&doc.transact());
                assert_eq!(actual, new, "seed={seed} old={old:?}");
                expected = new;
            }
        }
    }

    #[test]
    fn simulated_simultaneous_edits_last_save_wins() {
        for seed in [7_u64, 99, 1_337] {
            let doc = Doc::new();
            let ytext = doc.get_or_insert_text("content");
            let mut rng = Lcg::new(seed);
            let mut current = random_string(&mut rng, 10, 140);

            {
                let mut txn = doc.transact_mut();
                ytext.insert(&mut txn, 0, &current);
            }

            for _ in 0..120 {
                // Two "simultaneous" editors branch from the same base.
                let from = current.clone();
                let editor_a = mutate_text(&mut rng, &from);
                let editor_b = mutate_text(&mut rng, &from);

                // Save A, then rapid-save B: final state should be B.
                apply_text_diff_to_ytext(&doc, &ytext, &from, &editor_a);
                apply_text_diff_to_ytext(&doc, &ytext, &editor_a, &editor_b);
                let actual = ytext.get_string(&doc.transact());
                assert_eq!(actual, editor_b, "seed={seed} from={from:?}");
                current = editor_b;
            }
        }
    }

    #[test]
    #[ignore = "nightly: randomized large-diff stress coverage for diff-to-Yjs"]
    fn randomized_large_diffs_nightly() {
        for seed in [3_u64, 17, 404, 8_191] {
            let doc = Doc::new();
            let ytext = doc.get_or_insert_text("content");
            let mut rng = Lcg::new(seed);
            let mut expected = random_string(&mut rng, 4_000, 8_000);

            {
                let mut txn = doc.transact_mut();
                ytext.insert(&mut txn, 0, &expected);
            }

            for _ in 0..40 {
                let old = expected.clone();
                // Force heavy rewrites to exercise large replace ranges and UTF-8 offsets.
                let new = random_string(&mut rng, 3_000, 12_000);
                apply_text_diff_to_ytext(&doc, &ytext, &old, &new);
                let actual = ytext.get_string(&doc.transact());
                assert_eq!(actual, new, "seed={seed}");
                expected = new;
            }
        }
    }
}
