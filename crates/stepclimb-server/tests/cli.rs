// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary refuses bad configuration before it touches the network, and its
//! subcommands exit 0 on success, 2 on refused configuration and 1 when the database fails. The
//! `#[sqlx::test]` cases need `DATABASE_URL` (`just up`).

use std::path::PathBuf;
use std::process::{Command, Output};

use sqlx::{ConnectOptions, PgPool};
use stepclimb_core::db::MIGRATOR;

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

#[test]
fn a_wrong_type_in_the_file_exits_2_even_when_a_variable_overrides_it() {
    let file = TomlFile::new("overridden", "bind = 8080\n");
    let output = stepclimb()
        .args(["serve", "--config"])
        .arg(&file.0)
        .env("STEPCLIMB_BIND", "127.0.0.1:0")
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!(
            "`bind` ({} line 1): must be a string",
            file.0.display()
        )),
        "stderr: {stderr}"
    );
}

const KEYS: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
/// A database URL with a password, on a port nothing listens on.
const UNREACHABLE_URL: &str = "postgres://u:zq-db-secret@127.0.0.1:1/db";

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn check_config_exits_0_printing_only_non_secret_values_without_connecting() {
    let file = TomlFile::new("check-ok", "bind = \"0.0.0.0:9000\"\n");
    let output = stepclimb()
        .args(["check-config", "--config"])
        .arg(&file.0)
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert_eq!(
        stdout,
        format!(
            "stepclimb: the configuration is valid\nfile: {}\nbind: 0.0.0.0:9000\nlog: info\n",
            file.0.display()
        )
    );
    for secret in ["zq-db-secret", "AAEC"] {
        assert!(!stdout.contains(secret), "stdout: {stdout}");
        assert!(!stderr.contains(secret), "stderr: {stderr}");
    }
}

#[test]
fn check_config_without_a_file_says_so() {
    let output = stepclimb()
        .arg("check-config")
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    let stdout = stdout_of(&output);
    assert!(stdout.contains("\nfile: (none)\n"), "stdout: {stdout}");
    assert!(
        stdout.contains("\nbind: 127.0.0.1:8080\n"),
        "stdout: {stdout}"
    );
}

#[test]
fn check_config_with_a_missing_value_exits_2_naming_it() {
    let output = stepclimb()
        .arg("check-config")
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stdout_of(&output), "");
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("STEPCLIMB_ENCRYPTION_KEYS is not set"),
        "stderr: {stderr}"
    );
}

#[test]
fn check_config_with_a_toml_syntax_error_exits_2_naming_the_file_and_line() {
    let file = TomlFile::new("check-syntax", "log = \"info\"\nbind = 0.0.0.0\n");
    let output = stepclimb()
        .args(["check-config", "--config"])
        .arg(&file.0)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!("{} line 2: ", file.0.display())),
        "stderr: {stderr}"
    );
}

#[test]
fn check_config_with_an_unknown_key_exits_2_naming_the_file_and_line() {
    let file = TomlFile::new("check-unknown", "log = \"info\"\n\nbnid = \"0.0.0.0:80\"\n");
    let output = stepclimb()
        .arg("check-config")
        .env("STEPCLIMB_CONFIG", &file.0)
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!(
            "{} line 3: unknown field `bnid`",
            file.0.display()
        )),
        "stderr: {stderr}"
    );
}

#[test]
fn migrate_with_bad_configuration_exits_2_naming_it() {
    let output = stepclimb()
        .arg("migrate")
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("STEPCLIMB_DATABASE_URL is not set"),
        "stderr: {stderr}"
    );
}

#[test]
fn migrate_against_an_unreachable_database_exits_1() {
    let output = stepclimb()
        .arg("migrate")
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("stepclimb: could not connect to the database"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("zq-db-secret"), "stderr: {stderr}");
}

/// `stepclimb migrate` against the database behind `pool`.
fn migrate_binary(pool: &PgPool) -> Output {
    stepclimb()
        .arg("migrate")
        .env(
            "STEPCLIMB_DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs")
}

async fn applied_versions(pool: &PgPool) -> Vec<i64> {
    sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success ORDER BY version")
        .fetch_all(pool)
        .await
        .expect("the migrations table exists")
}

#[sqlx::test(migrations = false)]
async fn migrate_applies_every_migration_exits_0_and_is_idempotent(pool: PgPool) {
    let embedded: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
    assert!(!embedded.is_empty());

    let output = migrate_binary(&pool);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        stderr_of(&output).contains("database migrations are up to date"),
        "{}",
        stderr_of(&output)
    );
    assert_eq!(applied_versions(&pool).await, embedded);

    let again = migrate_binary(&pool);
    assert_eq!(again.status.code(), Some(0), "{}", stderr_of(&again));
    assert_eq!(applied_versions(&pool).await, embedded);
}

#[sqlx::test(migrations = false)]
async fn migrate_exits_1_when_a_migration_fails(pool: PgPool) {
    // The first migration creates `organizations`; a table already there makes it fail.
    sqlx::query("CREATE TABLE organizations (id integer)")
        .execute(&pool)
        .await
        .unwrap();
    let output = migrate_binary(&pool);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    let first = MIGRATOR.iter().next().expect("one migration").version;
    assert!(
        stderr.contains(&format!(
            "stepclimb: could not apply the database migrations: while executing migration \
             {first}: "
        )),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(r#"relation "organizations" already exists"#),
        "stderr: {stderr}"
    );
    assert_eq!(applied_versions(&pool).await, Vec::<i64>::new());
}

/// Every subcommand refuses a database URL sqlx would not read as written, before logging starts
/// or anything connects, without printing the value.
#[test]
fn every_subcommand_refuses_a_database_url_sqlx_would_not_read_as_written() {
    let cases = [
        (
            "postgres://u@127.0.0.1:1/db?sslpassword=zqsecret",
            "query parameter 1 of the URL is not one the server reads",
        ),
        (
            "postgres://u@127.0.0.1:1/db?password=ab&zqsecret",
            "query parameter 2 of the URL is not one the server reads",
        ),
        (
            "postgres://u:123/zqsecret@127.0.0.1:1/db",
            "has an @ after the host",
        ),
        (
            "postgres://u:zqsecret@127.0.0.1:notaport/db",
            "is not a valid PostgreSQL URL",
        ),
    ];
    for subcommand in ["check-config", "migrate", "serve"] {
        for (url, problem) in cases {
            let output = stepclimb()
                .arg(subcommand)
                .env("STEPCLIMB_DATABASE_URL", url)
                .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
                .output()
                .expect("the binary runs");
            let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
            assert_eq!(
                output.status.code(),
                Some(2),
                "{subcommand} {url}: {stderr}"
            );
            assert!(
                stderr.contains(&format!("STEPCLIMB_DATABASE_URL: {problem}")),
                "{subcommand}: {stderr}"
            );
            assert!(!stdout.contains("zqsecret"), "{subcommand}: {stdout}");
            assert!(!stderr.contains("zqsecret"), "{subcommand}: {stderr}");
        }
    }
}

#[test]
fn healthcheck_with_nothing_listening_exits_1_naming_the_address() {
    let free = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind a free port")
        .local_addr()
        .expect("local address");
    let output = stepclimb()
        .arg("healthcheck")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env(
            "STEPCLIMB_ENCRYPTION_KEYS",
            "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        )
        .env("STEPCLIMB_BIND", free.to_string())
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!(
            "stepclimb: healthcheck: could not connect to {free}"
        )),
        "stderr: {stderr}"
    );
}

#[test]
fn healthcheck_exits_0_when_readyz_answers_200() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a free port");
    let addr = listener.local_addr().expect("local address");
    let server = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer).expect("read the request");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
            .expect("answer");
    });
    let output = stepclimb()
        .arg("healthcheck")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env(
            "STEPCLIMB_ENCRYPTION_KEYS",
            "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        )
        .env("STEPCLIMB_BIND", addr.to_string())
        .output()
        .expect("the binary runs");
    server.join().expect("server thread");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
}
