// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary refuses bad configuration before it touches the network.

use std::process::Command;

fn stepclimb() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stepclimb"));
    for (key, _) in std::env::vars() {
        if key.starts_with("STEPCLIMB_") {
            command.env_remove(key);
        }
    }
    command
}

#[test]
fn serve_without_a_database_url_exits_2_naming_the_variable() {
    let output = stepclimb().arg("serve").output().expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("STEPCLIMB_DATABASE_URL is not set"),
        "stderr: {stderr}"
    );
}

#[test]
fn serve_with_a_bad_bind_address_exits_2_naming_it() {
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env("STEPCLIMB_BIND", "not-an-address")
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(r#"STEPCLIMB_BIND is "not-an-address""#),
        "stderr: {stderr}"
    );
}

#[test]
fn serve_without_encryption_keys_exits_2_naming_the_variable() {
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("STEPCLIMB_ENCRYPTION_KEYS is not set"),
        "stderr: {stderr}"
    );
}

#[test]
fn serve_with_a_bad_encryption_key_exits_2_without_printing_it() {
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env("STEPCLIMB_ENCRYPTION_KEYS", "k1:c2hvcnQtYnV0LXNlY3JldA==")
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(r#"entry 1 (id "k1")"#), "stderr: {stderr}");
    assert!(!stderr.contains("c2hvcnQ"), "stderr: {stderr}");
}
