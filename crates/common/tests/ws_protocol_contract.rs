use scriptum_common::protocol::ws::{CURRENT_PROTOCOL_VERSION, SUPPORTED_PROTOCOL_VERSIONS};

fn load_contract() -> serde_json::Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../contracts/ws-protocol.json");
    let content = std::fs::read_to_string(path).expect("contract file should be readable");
    serde_json::from_str(&content).expect("contract file should be valid JSON")
}

#[test]
fn current_version_matches_contract() {
    let contract = load_contract();
    let expected =
        contract["current_version"].as_str().expect("current_version should be a string");
    assert_eq!(CURRENT_PROTOCOL_VERSION, expected);
}

#[test]
fn supported_versions_match_contract() {
    let contract = load_contract();
    let expected: Vec<&str> = contract["protocol_versions"]
        .as_array()
        .expect("protocol_versions should be an array")
        .iter()
        .map(|v| v.as_str().expect("version should be a string"))
        .collect();
    assert_eq!(SUPPORTED_PROTOCOL_VERSIONS, &expected[..]);
}
