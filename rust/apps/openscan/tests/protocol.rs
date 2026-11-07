#![allow(clippy::expect_used)] // Test process and fixture helpers.
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn call(verb: &str, input: &[u8]) -> (bool, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_openscan"))
        .arg(verb)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("request");
    let output = child.wait_with_output().expect("wait");
    let value = serde_json::from_slice(&output.stdout).expect("JSON reply");
    (output.status.success(), value)
}
#[test]
fn binary_protocol_and_presets_work_without_a_scanner() {
    let (ok, v) = call("version", b"");
    assert!(ok);
    assert_eq!(v["protocolVersion"], 1);
    assert_eq!(v["implementation"], "Rust");
    let dir = tempfile::tempdir().expect("temp");
    let request = json!({"directory":dir.path(),"profile":{"name":"Synthetic preset","dpi":150}});
    assert!(call("preset-save", &serde_json::to_vec(&request).expect("JSON")).0);
    assert!(
        !call("preset-save", &serde_json::to_vec(&request).expect("JSON")).0,
        "no implicit overwrite"
    );
    let (ok, v) = call(
        "preset-list",
        &serde_json::to_vec(&json!({"directory":dir.path()})).expect("JSON"),
    );
    assert!(ok);
    assert_eq!(v["presets"][0]["dpi"], 150);
    assert!(
        call(
            "preset-delete",
            &serde_json::to_vec(&json!({"directory":dir.path(),"name":"Synthetic preset"}))
                .expect("JSON")
        )
        .0
    );
    let (_, v) = call(
        "preset-list",
        &serde_json::to_vec(&json!({"directory":dir.path()})).expect("JSON"),
    );
    assert_eq!(v["presets"], json!([]));
}
#[test]
fn invalid_requests_return_structured_errors_without_crashing() {
    for (verb, bytes) in [
        ("unknown", b"".as_slice()),
        ("preset-save", b"invalid JSON".as_slice()),
        ("capabilities", br#"{"deviceId":"not-a-driver"}"#.as_slice()),
    ] {
        let (ok, v) = call(verb, bytes);
        assert!(!ok);
        assert_eq!(v["ok"], false);
        assert_eq!(v["protocolVersion"], 1);
        assert!(v["error"].is_string());
    }
}
