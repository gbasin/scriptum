use std::collections::BTreeSet;

use scriptum_common::protocol::jsonrpc::SUPPORTED_PROTOCOL_VERSIONS;
use scriptum_common::protocol::rpc_methods::{IMPLEMENTED_METHODS, PLANNED_METHODS};

fn load_contract() -> serde_json::Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../contracts/jsonrpc-methods.json");
    let content = std::fs::read_to_string(path).expect("contract file should be readable");
    serde_json::from_str(&content).expect("contract file should be valid JSON")
}

#[test]
fn implemented_methods_match_contract() {
    let contract = load_contract();
    let expected: BTreeSet<&str> = contract["implemented_methods"]
        .as_array()
        .expect("implemented_methods should be an array")
        .iter()
        .map(|v| v.as_str().expect("method should be a string"))
        .collect();

    let actual: BTreeSet<&str> = IMPLEMENTED_METHODS.iter().copied().collect();
    assert_eq!(actual, expected, "IMPLEMENTED_METHODS diverged from contract");
}

#[test]
fn planned_methods_match_contract() {
    let contract = load_contract();
    let expected: BTreeSet<&str> = contract["planned_methods"]
        .as_array()
        .expect("planned_methods should be an array")
        .iter()
        .map(|v| v.as_str().expect("method should be a string"))
        .collect();

    let actual: BTreeSet<&str> = PLANNED_METHODS.iter().copied().collect();
    assert_eq!(actual, expected, "PLANNED_METHODS diverged from contract");
}

#[test]
fn rpc_protocol_versions_match_contract() {
    let contract = load_contract();
    let expected: Vec<&str> = contract["rpc_protocol_versions"]
        .as_array()
        .expect("rpc_protocol_versions should be an array")
        .iter()
        .map(|v| v.as_str().expect("version should be a string"))
        .collect();

    assert_eq!(
        SUPPORTED_PROTOCOL_VERSIONS,
        &expected[..],
        "SUPPORTED_PROTOCOL_VERSIONS diverged from contract"
    );
}

#[test]
fn no_overlap_between_implemented_and_planned() {
    let implemented: BTreeSet<&str> = IMPLEMENTED_METHODS.iter().copied().collect();
    let planned: BTreeSet<&str> = PLANNED_METHODS.iter().copied().collect();
    let overlap: Vec<&&str> = implemented.intersection(&planned).collect();
    assert!(overlap.is_empty(), "methods appear in both implemented and planned: {overlap:?}");
}
