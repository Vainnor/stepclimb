// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `stepclimb` binary refuses bad configuration before it touches the network, and its
//! subcommands exit 0 on success, 2 on refused configuration or a refused argument value and 1
//! when the database fails. The `#[sqlx::test]` cases need `DATABASE_URL` (`just up`).

use std::path::PathBuf;
use std::process::{Command, Output};

use sqlx::{ConnectOptions, PgPool};
use stepclimb_auth::token::TokenHash;
use stepclimb_core::{db::MIGRATOR, id::Uuid};

/// The binary with no `STEPCLIMB_*` variable from this process but a public URL, which every
/// test that is not about it would otherwise have to repeat.
fn stepclimb() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stepclimb"));
    for (key, _) in std::env::vars() {
        if key.starts_with("STEPCLIMB_") {
            command.env_remove(key);
        }
    }
    command.env("STEPCLIMB_PUBLIC_URL", "https://lms.example.org");
    command
}

#[test]
fn serve_without_a_public_url_exits_2_naming_the_variable() {
    let output = stepclimb()
        .arg("serve")
        .env_remove("STEPCLIMB_PUBLIC_URL")
        .env("STEPCLIMB_DATABASE_URL", "postgres://u@127.0.0.1:1/db")
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("STEPCLIMB_PUBLIC_URL is not set"),
        "stderr: {stderr}"
    );
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
            "stepclimb: the configuration is valid\nfile: {}\nbind: 0.0.0.0:9000\nlog: info\n\
             public_url: https://lms.example.org\nallowed_origins: (none)\n\
             trusted_proxies: (none)\nsign_in_rate_limit: 20\ntoken_rate_limit: 60\n\
             failed_sign_in_limit: 10\nallow_insecure_providers: false\nrole: all\n\
             worker_concurrency: 4\nmax_file_bytes: 104857600\n\
             max_video_bytes: 5368709120\nmax_video_seconds: 14400\n\
             transcode_concurrency: 1\ntranscode_threads: 0\ntranscode_dir: {}\n\
             ffmpeg_path: (PATH)\nffprobe_path: (PATH)\n\
             prlimit_path: (PATH)\nproviders: (none)\n\
             storage: not configured (STEPCLIMB_STORAGE_URL)\n",
            file.0.display(),
            std::env::temp_dir().display()
        )
    );
    for secret in ["zq-db-secret", "AAEC"] {
        assert!(!stdout.contains(secret), "stdout: {stdout}");
        assert!(!stderr.contains(secret), "stderr: {stderr}");
    }
}

/// docs/specs/m1/06-files.md T1: `check-config` prints the S3 settings, the secret access key
/// only as set, without connecting to the store.
#[test]
fn check_config_prints_the_storage_settings_with_the_secret_hidden() {
    let output = stepclimb()
        .arg("check-config")
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .env("STEPCLIMB_STORAGE_URL", "s3://stepclimb/lms")
        .env("STEPCLIMB_STORAGE_S3_ENDPOINT", "http://127.0.0.1:1")
        .env("STEPCLIMB_STORAGE_S3_ALLOW_HTTP", "true")
        .env("STEPCLIMB_STORAGE_S3_ACCESS_KEY_ID", "test-key")
        .env("STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY", "zq-s3-secret")
        .output()
        .expect("the binary runs");
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(
        stdout.ends_with(
            "storage_url: s3://stepclimb/lms\nstorage_s3_endpoint: http://127.0.0.1:1\n\
             storage_s3_region: us-east-1\nstorage_s3_access_key_id: test-key\n\
             storage_s3_secret_access_key: (set, hidden)\nstorage_s3_allow_http: true\n"
        ),
        "stdout: {stdout}"
    );
    assert!(!stdout.contains("zq-s3-secret"), "stdout: {stdout}");
    assert!(!stderr.contains("zq-s3-secret"), "stderr: {stderr}");
}

/// T1: S3 settings without `storage_url` stop `check-config` and `serve` with exit 2, naming the
/// setting, before anything connects.
#[test]
fn an_s3_setting_without_a_storage_url_exits_2_naming_it() {
    for command in ["check-config", "serve"] {
        let output = stepclimb()
            .arg(command)
            .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
            .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
            .env("STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY", "zq-s3-secret")
            .output()
            .expect("the binary runs");
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(2), "{command}: {stderr}");
        assert!(
            stderr.contains(
                "STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY: applies only to an s3:// storage URL, \
                 and STEPCLIMB_STORAGE_URL is not set"
            ),
            "{command}: {stderr}"
        );
        assert!(!stderr.contains("zq-s3-secret"), "{command}: {stderr}");
    }
}

/// T1: a store the server cannot write stops `serve` with exit 1 naming the probe's failed step,
/// before it connects to the database.
#[test]
fn serve_with_a_store_it_cannot_write_exits_1_naming_the_probe() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("stepclimb-cli-{}-read-only", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .env("STEPCLIMB_STORAGE_URL", format!("file://{}", dir.display()))
        .output()
        .expect("the binary runs");
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
    let _ = std::fs::remove_dir_all(&dir);
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains("stepclimb: the storage probe could not write .probe/"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("database"), "stderr: {stderr}");
}

/// T1: a directory that does not exist stops `serve` naming `STEPCLIMB_STORAGE_URL`.
#[test]
fn serve_with_a_missing_storage_directory_exits_1_naming_the_setting() {
    let output = stepclimb()
        .arg("serve")
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .env(
            "STEPCLIMB_STORAGE_URL",
            "file:///nonexistent/stepclimb-storage",
        )
        .output()
        .expect("the binary runs");
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains("stepclimb: could not open the store STEPCLIMB_STORAGE_URL names"),
        "stderr: {stderr}"
    );
}

/// Spec 06 B11's canonical `[[providers]]` example, its secret in a file beside it.
const PROVIDERS: &str = r#"
[[providers]]
kind = "oauth2"
key = "vatsim"
label = "Sign in with VATSIM"
authorize = "https://auth.vatsim.net/oauth/authorize"
token = "https://auth.vatsim.net/oauth/token"
userinfo = "https://auth.vatsim.net/api/user"
scopes = ["full_name", "vatsim_details", "email"]
client_id = "stepclimb-test-client"
client_secret_file = "vatsim_client_secret"

[providers.claims]
subject = "/data/cid"
given_name = "/data/personal/name_first"
family_name = "/data/personal/name_last"
email = "/data/personal/email"
attributes.rating = "/data/vatsim/rating/short"
attributes.subdivision = "/data/vatsim/subdivision/id"
"#;

const PROVIDER_SECRET: &str = "zq-vatsim-client-secret";

fn providers_file(test: &str, contents: &str) -> TomlFile {
    let file = TomlFile::new(test, contents);
    let dir = file.0.parent().expect("a directory");
    std::fs::write(dir.join("vatsim_client_secret"), PROVIDER_SECRET).expect("the secret file");
    file
}

/// Spec 06 T9: the canonical example loads, and only the provider's key and kind are printed.
#[test]
fn check_config_lists_the_files_providers_without_their_secret() {
    let file = providers_file("check-providers", PROVIDERS);
    let output = stepclimb()
        .args(["check-config", "--config"])
        .arg(&file.0)
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(
        stdout.contains("\nproviders: vatsim (oauth2)\n"),
        "stdout: {stdout}"
    );
    for hidden in [PROVIDER_SECRET, "vatsim_client_secret"] {
        assert!(!stdout.contains(hidden), "stdout: {stdout}");
        assert!(!stderr.contains(hidden), "stderr: {stderr}");
    }
}

/// Spec 06 T9 and B13: without `token`, `check-config` and `serve` both stop with exit 2 before
/// connecting, naming the key and the field.
#[test]
fn a_provider_without_a_token_exits_2_naming_the_key_and_the_field() {
    let file = providers_file(
        "providers-no-token",
        &PROVIDERS.replace("token = \"https://auth.vatsim.net/oauth/token\"\n", ""),
    );
    for command in ["check-config", "serve"] {
        let output = stepclimb()
            .args([command, "--config"])
            .arg(&file.0)
            .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
            .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
            .output()
            .expect("the binary runs");
        assert_eq!(output.status.code(), Some(2), "{command}");
        assert_eq!(stdout_of(&output), "", "{command}");
        let stderr = stderr_of(&output);
        assert_eq!(
            stderr,
            format!(
                "stepclimb: configuration error: {}: [[providers]] entry 1 (key \"vatsim\"): \
                 `token` is required\n",
                file.0.display()
            ),
            "{command}"
        );
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
    let admin_create = [
        "admin",
        "create",
        "--org-slug",
        "example",
        "--org-name",
        "Example",
        "--email",
        "sam@example.test",
        "--name",
        "Sam",
    ];
    let enrollment_link = [
        "admin",
        "enrollment-link",
        "--org",
        "example",
        "--user",
        "sam@example.test",
    ];
    let subcommands: [&[&str]; 5] = [
        &["check-config"],
        &["migrate"],
        &["serve"],
        &admin_create,
        &enrollment_link,
    ];
    for subcommand in subcommands {
        for (url, problem) in cases {
            let output = stepclimb()
                .args(subcommand)
                .env("STEPCLIMB_DATABASE_URL", url)
                .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
                .output()
                .expect("the binary runs");
            let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
            assert_eq!(
                output.status.code(),
                Some(2),
                "{subcommand:?} {url}: {stderr}"
            );
            assert!(
                stderr.contains(&format!("STEPCLIMB_DATABASE_URL: {problem}")),
                "{subcommand:?}: {stderr}"
            );
            assert!(!stdout.contains("zqsecret"), "{subcommand:?}: {stdout}");
            assert!(!stderr.contains("zqsecret"), "{subcommand:?}: {stderr}");
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

/// `stepclimb admin create` with `args` against the database behind `pool`.
fn admin_create_binary(pool: &PgPool, args: &[&str]) -> Output {
    stepclimb()
        .args(["admin", "create"])
        .args(args)
        .env(
            "STEPCLIMB_DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs")
}

/// The arguments of `admin create` for the organization `slug` and its admin.
fn create_args<'a>(slug: &'a str, email: &'a str) -> [&'a str; 8] {
    [
        "--org-slug",
        slug,
        "--org-name",
        "Example Training",
        "--email",
        email,
        "--name",
        "Sam Admin",
    ]
}

/// Every row of the tables `admin create` writes, as text in a fixed order: a snapshot that
/// shows whether a run changed anything.
async fn created_rows(pool: &PgPool) -> Vec<String> {
    let mut rows = Vec::new();
    for table in [
        "organizations",
        "accounts",
        "org_plans",
        "users",
        "role_assignments",
        "local_tokens",
        "roles",
        "plans",
    ] {
        let query = format!("SELECT '{table} ' || to_jsonb(t)::text FROM {table} t ORDER BY 1");
        let table_rows: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_all(pool)
            .await
            .expect("read the table");
        rows.extend(table_rows);
    }
    rows
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .expect("count rows")
}

/// T20 (spec 02 B27): on an empty database, one organization with its root account and plan
/// row, one user and one organization-scope Organization admin assignment.
#[sqlx::test(migrations = false)]
async fn admin_create_on_an_empty_database_creates_the_organization_and_its_admin(pool: PgPool) {
    let output = admin_create_binary(&pool, &create_args("example", "sam@example.test"));
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert_eq!(
        applied_versions(&pool).await,
        MIGRATOR.iter().map(|m| m.version).collect::<Vec<_>>()
    );

    let (org_id, slug, name): (Uuid, String, String) =
        sqlx::query_as("SELECT id, slug, name FROM organizations")
            .fetch_one(&pool)
            .await
            .expect("exactly one organization");
    assert_eq!(
        (slug.as_str(), name.as_str()),
        ("example", "Example Training")
    );
    let roots: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM accounts WHERE org_id = $1 AND parent_id IS NULL AND name = $2",
    )
    .bind(org_id)
    .bind("Example Training")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((roots, count(&pool, "accounts").await), (1, 1));
    let plan: (String, String) = sqlx::query_as(
        "SELECT p.key, op.source FROM org_plans op JOIN plans p ON p.id = op.plan_id
         WHERE op.org_id = $1",
    )
    .bind(org_id)
    .fetch_one(&pool)
    .await
    .expect("the organization's plan row");
    assert_eq!(plan, ("unlimited".to_owned(), "selfhost".to_owned()));

    let user: (Uuid, String, String, String, String, String, String) = sqlx::query_as(
        "SELECT id, email, display_name, source, status, tz, locale FROM users WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_one(&pool)
    .await
    .expect("exactly one user");
    assert_eq!(
        (&*user.1, &*user.2, &*user.3, &*user.4, &*user.5, &*user.6),
        (
            "sam@example.test",
            "Sam Admin",
            "local",
            "active",
            "UTC",
            "en"
        )
    );
    let assignment: (String, String, Uuid, String) = sqlx::query_as(
        "SELECT r.key, ra.scope_type, ra.scope_id, ra.source
         FROM role_assignments ra JOIN roles r ON r.id = ra.role_id
         WHERE ra.org_id = $1 AND ra.user_id = $2",
    )
    .bind(org_id)
    .bind(user.0)
    .fetch_one(&pool)
    .await
    .expect("exactly one assignment");
    assert_eq!(
        assignment,
        (
            "org_admin".to_owned(),
            "organization".to_owned(),
            org_id,
            "local".to_owned()
        )
    );
    assert_eq!(count(&pool, "users").await, 1);
    assert_eq!(count(&pool, "role_assignments").await, 1);
    // The built-in roles were written by the command itself: `serve` never ran.
    assert_eq!(
        count(&pool, "roles WHERE org_id IS NULL AND builtin").await,
        8
    );

    assert!(!stderr.contains("no sign-in method"), "stderr: {stderr}");
    // The test captures stdout, so it is no terminal: the link is a credential its logs keep.
    assert!(
        stderr.contains(
            "stepclimb: warning: stdout is not a terminal; the enrollment link it carries is an \
             admin credential for 7 days"
        ),
        "stderr: {stderr}"
    );
    let (expires_at, token_hash): (String, Vec<u8>) = sqlx::query_as(
        "SELECT to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),
                token_hash
         FROM local_tokens WHERE org_id = $1 AND user_id = $2 AND kind = 'enrollment'",
    )
    .bind(org_id)
    .bind(user.0)
    .fetch_one(&pool)
    .await
    .expect("exactly one enrollment link");
    let (report, link) = stdout.trim_end().rsplit_once('\n').expect("the link line");
    assert_eq!(
        format!("{report}\n"),
        format!(
            "stepclimb: created organization \"example\" ({org_id})\n\
             stepclimb: created user \"sam@example.test\" ({}) with the Organization admin role \
             at organization scope\n\
             stepclimb: open this one-time link to set up the admin's passkey or password; it \
             works once and expires at {expires_at}:\n",
            user.0
        )
    );
    // Spec 04 B6, Q7: the link opens the web app's enrollment page, and the database holds the
    // SHA-256 of its token, not the token.
    let token = link
        .strip_prefix("https://lms.example.org/enroll#token=")
        .unwrap_or_else(|| panic!("not an enrollment link: {link}"));
    assert_eq!(token.len(), 43, "{link}");
    assert_eq!(token_hash, TokenHash::of(token).as_bytes());
    assert!(!stderr.contains(token), "stderr: {stderr}");
    let password = pool
        .connect_options()
        .to_url_lossy()
        .password()
        .map(str::to_owned)
        .unwrap_or_default();
    if !password.is_empty() {
        assert!(!stdout.contains(&password), "stdout: {stdout}");
        assert!(!stderr.contains(&password), "stderr: {stderr}");
    }
}

/// `stepclimb admin enrollment-link` (spec 04 B10): a new link for the admin `admin create`
/// made, printed last on stdout with the stderr warning, replacing the first link; an unknown
/// user exits 2 and makes nothing.
#[sqlx::test(migrations = false)]
async fn admin_enrollment_link_prints_a_new_link_for_the_admin(pool: PgPool) {
    let created = admin_create_binary(&pool, &create_args("example", "sam@example.test"));
    assert_eq!(created.status.code(), Some(0), "{}", stderr_of(&created));
    let first = stdout_of(&created)
        .trim_end()
        .rsplit('\n')
        .next()
        .unwrap()
        .to_owned();

    let output =
        admin_enrollment_link_binary(&pool, &["--org", "example", "--user", "SAM@example.test"]);
    let (stdout, stderr) = (stdout_of(&output), stderr_of(&output));
    assert_eq!(output.status.code(), Some(0), "stderr: {stderr}");
    assert!(
        stderr.contains("stepclimb: warning: stdout is not a terminal"),
        "{stderr}"
    );
    let link = stdout.trim_end().rsplit('\n').next().unwrap();
    let token = link
        .strip_prefix("https://lms.example.org/enroll#token=")
        .unwrap_or_else(|| panic!("not an enrollment link: {stdout}"));
    assert_ne!(link, first);
    let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT token_hash FROM local_tokens")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(stored, [TokenHash::of(token).as_bytes().to_vec()]);
    assert!(!stderr.contains(token), "stderr: {stderr}");

    let before = created_rows(&pool).await;
    let refused =
        admin_enrollment_link_binary(&pool, &["--org", "example", "--user", "kim@example.test"]);
    assert_eq!(refused.status.code(), Some(2), "{}", stderr_of(&refused));
    assert!(
        stderr_of(&refused).contains(
            "stepclimb: admin enrollment-link: the organization has no user with this email or \
             id; no link was made"
        ),
        "{}",
        stderr_of(&refused)
    );
    assert_eq!(stdout_of(&refused), "");
    assert_eq!(created_rows(&pool).await, before);
}

/// `stepclimb admin enrollment-link` with `args` against the database behind `pool`.
fn admin_enrollment_link_binary(pool: &PgPool, args: &[&str]) -> Output {
    stepclimb()
        .args(["admin", "enrollment-link"])
        .args(args)
        .env(
            "STEPCLIMB_DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs")
}

/// While an organization exists the command refuses, whatever the slug, and changes nothing
/// (B27, decided by the owner, 13 Nov 2026): a second organization would make every
/// unauthenticated request answer 404 until spec 02 Q2's resolution by host exists. With the
/// same slug this is also T20's "run again".
#[sqlx::test(migrations = false)]
async fn admin_create_with_an_organization_present_exits_2_and_changes_nothing(pool: PgPool) {
    let first = admin_create_binary(&pool, &create_args("example", "sam@example.test"));
    assert_eq!(first.status.code(), Some(0), "{}", stderr_of(&first));
    let before = created_rows(&pool).await;

    for slug in ["example", "second"] {
        let again = admin_create_binary(&pool, &create_args(slug, "kim@example.test"));
        let stderr = stderr_of(&again);
        assert_eq!(again.status.code(), Some(2), "{slug}: {stderr}");
        assert!(
            stderr.contains(
                "stepclimb: admin create: the instance already has an organization; a second one \
                 cannot be created until requests are matched to organizations by host (spec 02 \
                 Q2); nothing was created"
            ),
            "{slug}: {stderr}"
        );
        assert!(!stderr.contains("no sign-in method"), "{slug}: {stderr}");
        assert_eq!(stdout_of(&again), "", "{slug}");
        assert_eq!(created_rows(&pool).await, before, "{slug}");
    }
}

/// A refused value exits 2 naming its flag, and leaves no organization behind: the email is
/// checked after the organization row is inserted, so this is a failure midway through the
/// transaction.
#[sqlx::test(migrations = false)]
async fn admin_create_refusing_a_value_exits_2_naming_the_flag_and_creates_nothing(pool: PgPool) {
    let cases: [(&[&str], &str); 5] = [
        (
            &create_args("example", "  "),
            "--email: an email cannot be empty",
        ),
        (
            &create_args("example", "sam\u{200b}@example.test"),
            "--email: an email cannot contain control or invisible formatting characters",
        ),
        (
            &create_args("Example", "sam@example.test"),
            "--org-slug: an organization slug must be",
        ),
        (
            &create_args("admin", "sam@example.test"),
            "--org-slug: the organization slug is reserved",
        ),
        (
            &[
                "--org-slug",
                "example",
                "--org-name",
                "Example",
                "--email",
                "sam@example.test",
                "--name",
                " ",
            ],
            "--name: a display name cannot be empty",
        ),
    ];
    for (args, problem) in cases {
        let output = admin_create_binary(&pool, args);
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(
            stderr.contains(&format!("stepclimb: admin create: {problem}")),
            "{args:?}: {stderr}"
        );
        assert!(stderr.contains("nothing was created"), "{stderr}");
        for table in [
            "organizations",
            "accounts",
            "org_plans",
            "users",
            "role_assignments",
        ] {
            assert_eq!(count(&pool, table).await, 0, "{args:?}: {table}");
        }
    }
}

#[test]
fn admin_create_without_a_required_argument_exits_2_naming_it() {
    let output = stepclimb()
        .args([
            "admin",
            "create",
            "--org-slug",
            "example",
            "--org-name",
            "Example",
        ])
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("--email <EMAIL>"), "stderr: {stderr}");
    assert!(stderr.contains("--name <NAME>"), "stderr: {stderr}");
}

#[test]
fn admin_create_against_an_unreachable_database_exits_1() {
    let output = stepclimb()
        .args(["admin", "create"])
        .args(create_args("example", "sam@example.test"))
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("stepclimb: admin create: could not connect to the database: "),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "; no organization was created, but the migrations or the built-in role and plan sync \
             may have run"
        ),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("zq-db-secret"), "stderr: {stderr}");
}

// Spec m1/01 T11: `check-config` prints `role` and `worker_concurrency` as set.
#[test]
fn check_config_prints_the_role_and_the_worker_concurrency() {
    let file = TomlFile::new("check-role", "role = \"worker\"\n");
    let output = stepclimb()
        .args(["check-config", "--config"])
        .arg(&file.0)
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .env("STEPCLIMB_WORKER_CONCURRENCY", "12")
        .output()
        .expect("the binary runs");
    let stdout = stdout_of(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout.contains("\nrole: worker\n"), "{stdout}");
    assert!(stdout.contains("\nworker_concurrency: 12\n"), "{stdout}");
}

// Spec m1/01 T11: a bad role or concurrency stops `check-config` with exit 2 naming it.
#[test]
fn check_config_refuses_a_bad_role_or_concurrency_naming_the_setting() {
    let role = TomlFile::new("check-bad-role", "role = \"other\"\n");
    let concurrency = TomlFile::new("check-bad-concurrency", "worker_concurrency = \"0\"\n");
    for (file, named) in [
        (&role, "`role` ("),
        (&concurrency, "`worker_concurrency` ("),
    ] {
        let output = stepclimb()
            .args(["check-config", "--config"])
            .arg(&file.0)
            .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
            .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
            .output()
            .expect("the binary runs");
        let stderr = stderr_of(&output);
        assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
        assert!(stderr.contains(named), "stderr: {stderr}");
    }
    let output = stepclimb()
        .arg("check-config")
        .env("STEPCLIMB_DATABASE_URL", UNREACHABLE_URL)
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .env("STEPCLIMB_WORKER_CONCURRENCY", "65")
        .output()
        .expect("the binary runs");
    let stderr = stderr_of(&output);
    assert_eq!(output.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("STEPCLIMB_WORKER_CONCURRENCY: 65 is more than 64"),
        "stderr: {stderr}"
    );
}

/// docs/specs/m1/06-files.md T1a: without `storage_url`, `serve` warns once naming
/// `STEPCLIMB_STORAGE_URL`, starts, and answers `/readyz` 200.
#[sqlx::test(migrations = false)]
async fn serve_without_storage_warns_once_and_is_ready(pool: PgPool) {
    use std::io::{Read as _, Write as _};
    let addr = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("its address");
    let mut child = stepclimb()
        .arg("serve")
        .env(
            "STEPCLIMB_DATABASE_URL",
            pool.connect_options().to_url_lossy().as_str(),
        )
        .env("STEPCLIMB_ENCRYPTION_KEYS", KEYS)
        .env("STEPCLIMB_BIND", addr.to_string())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the binary starts");
    let mut ready = None;
    for _ in 0..300 {
        if let Ok(mut stream) = std::net::TcpStream::connect(addr) {
            stream
                .write_all(b"GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .expect("send");
            let mut answer = String::new();
            let _ = stream.read_to_string(&mut answer);
            ready = answer.lines().next().map(str::to_owned);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    child.kill().expect("stop the server");
    let output = child.wait_with_output().expect("its output");
    let stderr = stderr_of(&output);
    assert_eq!(
        ready.as_deref(),
        Some("HTTP/1.1 200 OK"),
        "stderr: {stderr}"
    );
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("object storage is not configured"))
        .collect();
    assert_eq!(warnings.len(), 1, "stderr: {stderr}");
    assert!(warnings[0].contains(r#""level":"WARN""#), "{}", warnings[0]);
    assert!(
        warnings[0].contains("STEPCLIMB_STORAGE_URL"),
        "{}",
        warnings[0]
    );
}
