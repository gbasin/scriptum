use scriptum_common::diff::patch::{apply_text_diff_to_ytext, TextPatchOp};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use yrs::{Doc, GetString, Text, Transact};

#[derive(Debug)]
struct GoldenCase {
    name: String,
    before: String,
    after: String,
    expected_ops: Vec<ExpectedPatchOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ExpectedPatchOp {
    Insert { index: u32, text: String },
    Delete { index: u32, len: u32 },
}

impl From<ExpectedPatchOp> for TextPatchOp {
    fn from(value: ExpectedPatchOp) -> Self {
        match value {
            ExpectedPatchOp::Insert { index, text } => TextPatchOp::Insert { index, text },
            ExpectedPatchOp::Delete { index, len } => TextPatchOp::Delete { index, len },
        }
    }
}

#[test]
fn diff_to_yjs_golden_cases() {
    let cases_dir = golden_cases_dir();
    let cases = load_cases(&cases_dir);

    assert!(
        !cases.is_empty(),
        "no golden cases found in {}",
        cases_dir.display()
    );

    let mut failures = Vec::new();
    for case in cases {
        if let Err(message) = run_case(&case) {
            failures.push(message);
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} golden case(s) failed:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}

fn golden_cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/cases")
}

fn load_cases(cases_dir: &Path) -> Vec<GoldenCase> {
    let mut case_dirs: Vec<PathBuf> = fs::read_dir(cases_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", cases_dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_dir() { Some(path) } else { None }
        })
        .collect();
    case_dirs.sort();
    case_dirs.into_iter().map(load_case).collect()
}

fn load_case(case_dir: PathBuf) -> GoldenCase {
    let name = case_dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("<unnamed-case>")
        .to_owned();

    let before_path = case_dir.join("before.md");
    let after_path = case_dir.join("after.md");
    let expected_ops_path = case_dir.join("expected_ops.json");

    let before = read_required(&before_path);
    let after = read_required(&after_path);
    let expected_ops = serde_json::from_str::<Vec<ExpectedPatchOp>>(&read_required(&expected_ops_path))
        .unwrap_or_else(|error| {
            panic!(
                "failed to parse expected ops in {}: {error}",
                expected_ops_path.display()
            )
        });

    GoldenCase { name, before, after, expected_ops }
}

fn run_case(case: &GoldenCase) -> Result<(), String> {
    let doc = Doc::new();
    let ytext = doc.get_or_insert_text("content");
    {
        let mut txn = doc.transact_mut();
        ytext.insert(&mut txn, 0, &case.before);
    }

    let actual_ops = apply_text_diff_to_ytext(&doc, &ytext, &case.before, &case.after);
    let actual_after = ytext.get_string(&doc.transact());
    let expected_ops: Vec<TextPatchOp> =
        case.expected_ops.clone().into_iter().map(Into::into).collect();

    if actual_after != case.after {
        return Err(format!(
            "case `{}` final markdown mismatch.\nexpected: {:?}\nactual:   {:?}",
            case.name, case.after, actual_after
        ));
    }

    if actual_ops != expected_ops {
        return Err(format_ops_mismatch(&case.name, &expected_ops, &actual_ops));
    }

    Ok(())
}

fn format_ops_mismatch(case_name: &str, expected: &[TextPatchOp], actual: &[TextPatchOp]) -> String {
    let expected_rendered = render_ops(expected);
    let actual_rendered = render_ops(actual);
    let max_len = expected_rendered.len().max(actual_rendered.len());

    let mut diff_lines = Vec::with_capacity(max_len);
    for index in 0..max_len {
        let expected_line = expected_rendered.get(index).map(String::as_str).unwrap_or("<none>");
        let actual_line = actual_rendered.get(index).map(String::as_str).unwrap_or("<none>");
        let marker = if expected_line == actual_line { " " } else { "!" };
        diff_lines.push(format!(
            "{marker} [{index}] expected: {expected_line}\n      actual:   {actual_line}"
        ));
    }

    format!(
        "case `{case_name}` patch ops mismatch.\nExpected ops:\n{}\nActual ops:\n{}\nDiff:\n{}",
        expected_rendered.join("\n"),
        actual_rendered.join("\n"),
        diff_lines.join("\n")
    )
}

fn render_ops(ops: &[TextPatchOp]) -> Vec<String> {
    ops.iter()
        .map(|op| match op {
            TextPatchOp::Insert { index, text } => {
                format!("insert(index={index}, text={text:?})")
            }
            TextPatchOp::Delete { index, len } => {
                format!("delete(index={index}, len={len})")
            }
        })
        .collect()
}

fn read_required(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}
