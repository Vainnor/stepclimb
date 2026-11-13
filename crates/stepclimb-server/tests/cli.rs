// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary refuses bad configuration before it touches the network.

use std::path::PathBuf;
use std::process::{Command, Output};

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
        stderr.contains(r#"STEPCLIMB_BIND: "not-an-address" is not a socket address"#),
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

#[test]
fn serve_with_the_development_key_warns_naming_its_id() {
    let keys_file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/compose/development-encryption-keys"
    );
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env("STEPCLIMB_ENCRYPTION_KEYS_FILE", keys_file)
        .output()
        .expect("the binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let warning = stderr
        .lines()
        .find(|line| line.contains("published development key"))
        .unwrap_or_else(|| panic!("no warning in stderr: {stderr}"));
    assert!(warning.contains(r#""level":"WARN""#), "{warning}");
    assert!(warning.contains(r#""key_id":"local""#), "{warning}");
}

/// A `stepclimb.toml` with `contents` in a directory of its own, removed on drop.
struct TomlFile(PathBuf);

impl TomlFile {
    fn new(test: &str, contents: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("stepclimb-cli-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("stepclimb.toml");
        std::fs::write(&path, contents).expect("write the file");
        Self(path)
    }
}

impl Drop for TomlFile {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_toml_syntax_error_exits_2_naming_the_file_and_line() {
    let file = TomlFile::new(
        "syntax",
        "bind = \"0.0.0.0:8080\"\ndatabase_url = \"postgres://u:zq-secret@h/db\n",
    );
    let output = stepclimb()
        .args(["serve", "--config"])
        .arg(&file.0)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!("{} line 2: ", file.0.display())),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("zq-secret"), "stderr: {stderr}");
}

#[test]
fn an_unknown_toml_key_in_the_file_stepclimb_config_names_exits_2() {
    let file = TomlFile::new("unknown-key", "\ndatabse_url = \"postgres://u@h/db\"\n");
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_CONFIG", &file.0)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!(
            "{} line 2: unknown field `databse_url`",
            file.0.display()
        )),
        "stderr: {stderr}"
    );
}

#[test]
fn a_value_from_the_file_is_used_and_the_environment_overrides_it() {
    let file = TomlFile::new(
        "precedence",
        "bind = \"not-an-address\"\nlog = \"also-bad=x\"\n",
    );
    let output = stepclimb()
        .args(["serve", "--config"])
        .arg(&file.0)
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env(
            "STEPCLIMB_ENCRYPTION_KEYS",
            "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        )
        .env("STEPCLIMB_BIND", "127.0.0.1:0")
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!("`log` ({} line 2): ", file.0.display())),
        "stderr: {stderr}"
    );
}

#[test]
fn a_missing_config_file_exits_2_naming_its_path() {
    let output = stepclimb()
        .args(["serve", "--config", "/nonexistent/stepclimb.toml"])
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("could not read the configuration file /nonexistent/stepclimb.toml"),
        "stderr: {stderr}"
    );
}

#[test]
fn an_unknown_stepclimb_variable_exits_2_naming_it() {
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env(
            "STEPCLIMB_ENCRYPTION_KEY",
            "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        )
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("STEPCLIMB_ENCRYPTION_KEY is not a Stepclimb setting"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("AAEC"), "stderr: {stderr}");
}
