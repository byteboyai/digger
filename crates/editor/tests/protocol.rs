//! Shared JSON contract between the Rust bridge and the TypeScript sync state machine.
//!
//! The same file (`tests/protocol-fixtures.json`) is read by `crates/desktop/web/tests/protocol.test.mjs`.
//! If either side changes a field, error code or message shape, one of the two tests fails.

use digger_editor::protocol::{EditorInbound, Envelope, ErrorCode, PROTOCOL_VERSION};
use serde_json::Value;

const FIXTURES: &str = include_str!("protocol-fixtures.json");

fn fixtures() -> Value {
    serde_json::from_str(FIXTURES).expect("fixture JSON is valid")
}

#[test]
fn the_declared_protocol_version_matches_the_rust_constant() {
    assert_eq!(
        fixtures()["protocol_version"].as_u64(),
        Some(u64::from(PROTOCOL_VERSION))
    );
}

#[test]
fn every_error_code_round_trips_through_the_wire_form() {
    let declared: Vec<String> = fixtures()["error_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let all = [
        ErrorCode::StaleBase,
        ErrorCode::BadPosition,
        ErrorCode::MalformedChanges,
        ErrorCode::Unauthorized,
        ErrorCode::UnsupportedProtocol,
        ErrorCode::DuplicateRequest,
        ErrorCode::OutOfOrderRequest,
        ErrorCode::Internal,
    ];
    let mut seen = vec![];
    for code in all {
        let wire = serde_json::to_value(code).unwrap();
        let name = wire.as_str().unwrap().to_string();
        assert!(declared.contains(&name), "{name} missing from the fixture");
        let back: ErrorCode = serde_json::from_value(wire).unwrap();
        assert_eq!(back, code);
        seen.push(name);
    }
    assert_eq!(seen.len(), declared.len());
}

#[test]
fn inbound_fixtures_deserialize_into_the_typed_messages() {
    for case in fixtures()["inbound"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let raw = serde_json::to_string(&case["json"]).unwrap();
        let env: Envelope = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name}: {e}"));
        match name {
            "ready" => {
                assert!(matches!(
                    env.msg,
                    EditorInbound::Ready {
                        protocol: Some(2),
                        ..
                    }
                ));
            }
            "edit-with-request-id" => match env.msg {
                EditorInbound::Edit {
                    request_id,
                    base,
                    changes,
                } => {
                    assert_eq!(request_id, Some(7));
                    assert_eq!(base, 1);
                    assert_eq!(changes.len(), 1);
                }
                other => panic!("{name}: {other:?}"),
            },
            "snapshot" => match env.msg {
                EditorInbound::Snapshot { request_id } => assert_eq!(request_id, Some(8)),
                other => panic!("{name}: {other:?}"),
            },
            other => panic!("unexpected fixture {other}"),
        }
    }
}

#[test]
fn outbound_fixtures_are_the_shapes_the_page_validates() {
    for case in fixtures()["outbound"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let v = &case["json"];
        match name {
            "stale-reject" => {
                let code: ErrorCode = serde_json::from_value(v["code"].clone()).unwrap();
                assert_eq!(code, ErrorCode::StaleBase);
                assert_eq!(v["request_id"], 7);
                assert_eq!(v["version"], 2);
            }
            "snapshot-ok" => {
                for key in ["request_id", "text", "version", "hash", "len16", "blocks"] {
                    assert!(!v[key].is_null(), "{name}.{key}");
                }
            }
            "blocks-with-request-id" => {
                for key in ["request_id", "version", "hash", "len16", "blocks"] {
                    assert!(!v[key].is_null(), "{name}.{key}");
                }
            }
            "incremental-with-request-id" => {
                use digger_editor::protocol::{BlockInfo, Change, EditorOutbound};
                let reply = EditorOutbound::Incremental {
                    request_id: Some(7),
                    base_version: 1,
                    version: 2,
                    hash: String::new(),
                    len16: 5,
                    changes: vec![Change {
                        from: 2,
                        to: 2,
                        insert: "x".into(),
                    }],
                    removed_from: 0,
                    removed_to: 1,
                    blocks: vec![BlockInfo {
                        id: "1".into(),
                        kind: "paragraph",
                        level: None,
                        title: None,
                        how: "Similar",
                        from: 0,
                        to: 5,
                        head: "abxcd".into(),
                    }],
                };
                assert_eq!(serde_json::to_value(reply).unwrap(), *v);
            }
            other => panic!("unexpected fixture {other}"),
        }
    }
}
