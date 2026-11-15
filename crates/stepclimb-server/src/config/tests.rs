// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

const KEYS: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const URL: &str = "postgres://u@h/db";
const PUBLIC: &str = "https://lms.example.org";

/// A valid value for each secret setting, and the part of it no output may show.
const SECRET_SAMPLES: &[(Setting, &str, &str)] = &[
    (
        DATABASE_URL,
        "postgres://app:zq-db-secret@db.internal:5432/stepclimb",
        "zq-db-secret",
    ),
    (
        ENCRYPTION_KEYS,
        "s1:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=",
        "ICEiIyQlJico",
    ),
];

fn env(pairs: &[(&str, &str)]) -> Env {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// `pairs` plus a database URL, a key ring and a public URL, unless `pairs` sets one of their
/// variables.
fn with_required(pairs: &[(&str, &str)]) -> Env {
    let mut all = env(pairs);
    for (setting, value) in [
        (DATABASE_URL, URL),
        (ENCRYPTION_KEYS, KEYS),
        (PUBLIC_URL, PUBLIC),
    ] {
        if !pairs.iter().any(|(k, _)| k.starts_with(setting.var)) {
            all.insert(setting.var.to_owned(), value.to_owned());
        }
    }
    all
}

fn toml(text: &str) -> ConfigFile {
    ConfigFile::parse(
        PathBuf::from("/etc/stepclimb/stepclimb.toml"),
        text.to_owned(),
    )
    .unwrap()
}

fn toml_error(text: &str) -> ConfigError {
    ConfigFile::parse(
        PathBuf::from("/etc/stepclimb/stepclimb.toml"),
        text.to_owned(),
    )
    .unwrap_err()
}

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, contents: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("stepclimb-config-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn key_origin(key: &str, line: usize) -> Origin {
    Origin::Key {
        key: key.to_owned(),
        path: PathBuf::from("/etc/stepclimb/stepclimb.toml"),
        line,
    }
}

fn var(name: &str) -> Origin {
    Origin::Var(name.to_owned())
}

// Environment only.

#[test]
fn reads_the_database_url_and_defaults_the_bind_address_and_log_filter() {
    let config = Config::load(&with_required(&[]), None).unwrap();
    assert_eq!(config.database_url, URL);
    assert_eq!(config.bind, DEFAULT_BIND.parse::<SocketAddr>().unwrap());
    assert_eq!(config.log, DEFAULT_LOG);
    assert_eq!(config.file, None);
}

#[test]
fn reads_the_bind_address_and_log_filter() {
    let config = Config::load(
        &with_required(&[
            ("STEPCLIMB_BIND", "0.0.0.0:9000"),
            ("STEPCLIMB_LOG", "stepclimb_server=debug,info"),
        ]),
        None,
    )
    .unwrap();
    assert_eq!(config.bind, "0.0.0.0:9000".parse::<SocketAddr>().unwrap());
    assert_eq!(config.log, "stepclimb_server=debug,info");
}

#[test]
fn a_missing_database_url_is_refused_by_name_with_an_example() {
    let error = Config::load(&env(&[("STEPCLIMB_ENCRYPTION_KEYS", KEYS)]), None).unwrap_err();
    assert_eq!(error, ConfigError::Missing(DATABASE_URL));
    assert_eq!(
        error.to_string(),
        "STEPCLIMB_DATABASE_URL is not set (set it, STEPCLIMB_DATABASE_URL_FILE to a file \
         holding it, or `database_url` in the configuration file); for example \
         postgres://stepclimb@db:5432/stepclimb"
    );
}

#[test]
fn an_empty_variable_counts_as_unset() {
    let error = Config::load(&with_required(&[("STEPCLIMB_DATABASE_URL", "")]), None).unwrap_err();
    assert_eq!(error, ConfigError::Missing(DATABASE_URL));
    let config = Config::load(&with_required(&[("STEPCLIMB_BIND", "")]), None).unwrap();
    assert_eq!(config.bind, DEFAULT_BIND.parse::<SocketAddr>().unwrap());
}

#[test]
fn a_bad_bind_address_is_refused_with_its_value_and_an_example() {
    let error = Config::load(&with_required(&[("STEPCLIMB_BIND", "localhost")]), None).unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"STEPCLIMB_BIND: "localhost" is not a socket address; for example 0.0.0.0:8080"#
    );
}

#[test]
fn a_bad_log_filter_is_refused() {
    let error = Config::load(&with_required(&[("STEPCLIMB_LOG", "info,a=b=c")]), None).unwrap_err();
    assert_eq!(
        error,
        ConfigError::Invalid {
            origin: var("STEPCLIMB_LOG"),
            problem: format!(
                r#""info,a=b=c" is not a log filter; for example {}"#,
                LOG.example
            ),
        }
    );
}

#[test]
fn a_non_postgres_url_is_refused_without_repeating_it() {
    let error = Config::load(
        &with_required(&[("STEPCLIMB_DATABASE_URL", "mysql://u:zq@h/db")]),
        None,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.starts_with("STEPCLIMB_DATABASE_URL: the URL must start with postgres://"),
        "{message}"
    );
    assert!(!message.contains("zq"), "{message}");
}

#[test]
fn an_unknown_variable_is_refused_by_name() {
    let error = Config::load(
        &with_required(&[("STEPCLIMB_DATABSE_URL_FILE", "/run/secrets/db")]),
        None,
    )
    .unwrap_err();
    assert_eq!(
        error,
        ConfigError::UnknownVar {
            name: "STEPCLIMB_DATABSE_URL_FILE".to_owned()
        }
    );
    let message = error.to_string();
    assert!(message.contains("STEPCLIMB_DATABASE_URL_FILE"), "{message}");
    assert!(!message.contains("/run/secrets"), "{message}");
}

#[test]
fn every_known_variable_is_accepted() {
    let mut all = with_required(&[]);
    all.insert(
        CONFIG_VAR.to_owned(),
        "/etc/stepclimb/stepclimb.toml".to_owned(),
    );
    all.insert(BIND.var.to_owned(), "0.0.0.0:8080".to_owned());
    all.insert(LOG.var.to_owned(), "info".to_owned());
    Config::load(&all, None).unwrap();
    assert_eq!(
        known_vars(),
        [
            "STEPCLIMB_BIND",
            "STEPCLIMB_LOG",
            "STEPCLIMB_DATABASE_URL",
            "STEPCLIMB_DATABASE_URL_FILE",
            "STEPCLIMB_ENCRYPTION_KEYS",
            "STEPCLIMB_ENCRYPTION_KEYS_FILE",
            "STEPCLIMB_PUBLIC_URL",
            "STEPCLIMB_ALLOWED_ORIGINS",
            "STEPCLIMB_TRUSTED_PROXIES",
            "STEPCLIMB_SIGN_IN_RATE_LIMIT",
            "STEPCLIMB_TOKEN_RATE_LIMIT",
            "STEPCLIMB_FAILED_SIGN_IN_LIMIT",
            "STEPCLIMB_ALLOW_INSECURE_PROVIDERS",
            "STEPCLIMB_CONFIG",
        ]
    );
}

#[test]
fn the_config_flag_wins_over_the_variable() {
    let both = env(&[(CONFIG_VAR, "/from/env.toml")]);
    assert_eq!(
        config_path(Some(Path::new("/from/flag.toml")), &both),
        Some(PathBuf::from("/from/flag.toml"))
    );
    assert_eq!(
        config_path(None, &both),
        Some(PathBuf::from("/from/env.toml"))
    );
    assert_eq!(config_path(None, &env(&[(CONFIG_VAR, "")])), None);
    assert_eq!(config_path(None, &Env::new()), None);
}

// T1: the environment over the file, key by key.

#[test]
fn a_variable_wins_over_the_file_and_a_key_only_in_the_file_is_used() {
    let file = toml("bind = \"0.0.0.0:7000\"\nlog = \"debug\"\n");
    let config = Config::load(
        &with_required(&[("STEPCLIMB_BIND", "0.0.0.0:9000")]),
        Some(&file),
    )
    .unwrap();
    assert_eq!(config.bind, "0.0.0.0:9000".parse::<SocketAddr>().unwrap());
    assert_eq!(config.log, "debug");
    assert_eq!(
        config.file,
        Some(PathBuf::from("/etc/stepclimb/stepclimb.toml"))
    );
}

#[test]
fn a_secret_in_the_environment_wins_over_the_file_whichever_variant_each_uses() {
    let url_file = TempFile::new("env-url", "postgres://from-env@h/db\n");
    let file = toml(&format!(
        "database_url = \"postgres://from-file@h/db\"\nencryption_keys = \"{KEYS}\"\n"
    ));
    let config = Config::load(
        &env(&[
            ("STEPCLIMB_DATABASE_URL_FILE", url_file.path()),
            ("STEPCLIMB_PUBLIC_URL", PUBLIC),
        ]),
        Some(&file),
    )
    .unwrap();
    assert_eq!(config.database_url, "postgres://from-env@h/db");
    assert_eq!(config.encryption_keys.current_key_id(), "k1");
}

#[test]
fn the_file_alone_configures_the_server() {
    let file = toml(&format!(
        "bind = \"0.0.0.0:7000\"\ndatabase_url = \"{URL}\"\nencryption_keys = \"{KEYS}\"\n\
         public_url = \"{PUBLIC}\"\n"
    ));
    let config = Config::load(&Env::new(), Some(&file)).unwrap();
    assert_eq!(config.bind, "0.0.0.0:7000".parse::<SocketAddr>().unwrap());
    assert_eq!(config.database_url, URL);
}

#[test]
fn a_bad_value_in_the_file_names_the_key_file_and_line() {
    let file = toml("# Stepclimb\n\nbind = \"localhost\"\n");
    let error = Config::load(&with_required(&[]), Some(&file)).unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"`bind` (/etc/stepclimb/stepclimb.toml line 3): "localhost" is not a socket address; for example 0.0.0.0:8080"#
    );
}

#[test]
fn a_value_that_is_not_a_string_is_refused_without_repeating_it() {
    let error = toml_error("database_url = 31337\n");
    assert_eq!(
        error,
        ConfigError::Invalid {
            origin: key_origin("database_url", 1),
            problem: "must be a string in quotes".to_owned(),
        }
    );
    assert!(!error.to_string().contains("31337"), "{error}");
}

#[test]
fn the_whole_file_is_checked_even_where_a_variable_overrides_it() {
    let error = toml_error("log = \"info\"\nbind = 8080\n");
    assert_eq!(
        error,
        ConfigError::Invalid {
            origin: key_origin("bind", 2),
            problem: "must be a string in quotes".to_owned(),
        }
    );
    let error = toml_error(&format!(
        "database_url = \"{URL}\"\ndatabase_url_file = \"/run/secrets/db\"\n"
    ));
    assert_eq!(
        error,
        ConfigError::Twice {
            value: key_origin("database_url", 1),
            file: key_origin("database_url_file", 2),
        }
    );
}

#[test]
fn an_empty_string_in_the_file_counts_as_unset() {
    let file = toml("bind = \"\"\nlog = \"\"\n");
    let config = Config::load(&with_required(&[]), Some(&file)).unwrap();
    assert_eq!(config.bind, DEFAULT_BIND.parse::<SocketAddr>().unwrap());
    assert_eq!(config.log, DEFAULT_LOG);
    let file = toml("database_url = \"\"\n");
    let error =
        Config::load(&env(&[("STEPCLIMB_ENCRYPTION_KEYS", KEYS)]), Some(&file)).unwrap_err();
    assert_eq!(error, ConfigError::Missing(DATABASE_URL));
}

#[test]
fn debug_of_the_file_shows_its_path_and_key_names_only() {
    let (_, value, hidden) = SECRET_SAMPLES[0];
    let file = toml(&format!(
        "bind = \"0.0.0.0:7000\"\ndatabase_url = \"{value}\"\n"
    ));
    let printed = format!("{file:?}");
    assert_eq!(
        printed,
        r#"ConfigFile { path: "/etc/stepclimb/stepclimb.toml", keys: ["bind", "database_url"], .. }"#
    );
    assert!(!printed.contains(hidden), "{printed}");
}

#[test]
fn a_relative_file_key_resolves_against_the_config_files_directory() {
    let secret = TempFile::new("relative", &format!("{URL}\n"));
    let dir = secret.0.parent().unwrap();
    let text = "database_url_file = \"relative\"\n".to_owned();
    let file = ConfigFile::parse(dir.join("stepclimb.toml"), text).unwrap();
    let config = Config::load(
        &env(&[
            ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
            ("STEPCLIMB_PUBLIC_URL", PUBLIC),
        ]),
        Some(&file),
    )
    .unwrap();
    assert_eq!(config.database_url, URL);

    // A variable's relative path is relative to the working directory, not the file's.
    let error = Config::load(
        &env(&[
            ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
            ("STEPCLIMB_DATABASE_URL_FILE", "relative"),
        ]),
        Some(&file),
    )
    .unwrap_err();
    assert!(
        matches!(&error, ConfigError::File { origin: Origin::Var(name), .. } if name == "STEPCLIMB_DATABASE_URL_FILE"),
        "{error}"
    );
}

// T2: every secret has a `_FILE` variant, in the environment and in the file.

#[test]
fn every_secret_setting_has_a_sample_in_these_tests() {
    let secrets: Vec<Setting> = SETTINGS.iter().copied().filter(|s| s.secret).collect();
    let sampled: Vec<Setting> = SECRET_SAMPLES.iter().map(|(s, _, _)| *s).collect();
    assert_eq!(secrets, sampled, "add the new secret to SECRET_SAMPLES");
}

/// Every field of `Config` maps to a setting; adding a field fails to compile here until it is
/// classified, and a secret field must be a secret setting with a sample above.
#[test]
fn every_config_field_is_a_classified_setting() {
    let config = Config::load(&with_required(&[]), None).unwrap();
    let Config {
        file: _,
        bind: _,
        log: _,
        database_url: _,
        encryption_keys: _,
        public_url: _,
        allowed_origins: _,
        trusted_proxies: _,
        sign_in_rate_limit: _,
        token_rate_limit: _,
        failed_sign_in_limit: _,
        allow_insecure_providers: _,
        // `[[providers]]` is a record of the file, not a setting (`providers::tests`).
        providers: _,
    } = config;
    let fields = [
        (BIND, false),
        (LOG, false),
        (DATABASE_URL, true),
        (ENCRYPTION_KEYS, true),
        (PUBLIC_URL, false),
        (ALLOWED_ORIGINS, false),
        (TRUSTED_PROXIES, false),
        (SIGN_IN_RATE_LIMIT, false),
        (TOKEN_RATE_LIMIT, false),
        (FAILED_SIGN_IN_LIMIT, false),
        (ALLOW_INSECURE_PROVIDERS, false),
    ];
    for (setting, secret) in fields {
        assert!(
            SETTINGS.contains(&setting),
            "{} is not in SETTINGS",
            setting.var
        );
        assert_eq!(setting.secret, secret, "{}", setting.var);
    }
    assert_eq!(fields.len(), SETTINGS.len());
}

/// The environment with the public URL and every secret but `except` set directly.
fn other_secrets(except: Setting) -> Env {
    let mut vars: Env = SECRET_SAMPLES
        .iter()
        .filter(|(s, _, _)| *s != except)
        .map(|(s, v, _)| (s.var.to_owned(), (*v).to_owned()))
        .collect();
    vars.insert(PUBLIC_URL.var.to_owned(), PUBLIC.to_owned());
    vars
}

#[test]
fn every_secret_is_read_and_trimmed_from_its_file_variable() {
    for (setting, value, hidden) in SECRET_SAMPLES {
        let secret = TempFile::new(setting.key, &format!("  {value}\n"));
        let mut vars = other_secrets(*setting);
        vars.insert(format!("{}_FILE", setting.var), secret.path().to_owned());
        let config = Config::load(&vars, None).unwrap();
        let printed = format!("{config:?}");
        assert!(!printed.contains(hidden), "{}: {printed}", setting.var);
        match *setting {
            DATABASE_URL => assert_eq!(config.database_url, *value),
            ENCRYPTION_KEYS => assert_eq!(config.encryption_keys.current_key_id(), "s1"),
            _ => panic!("no check for {}", setting.var),
        }
    }
}

#[test]
fn every_secret_is_read_and_trimmed_from_its_file_key() {
    for (setting, value, _) in SECRET_SAMPLES {
        let secret = TempFile::new(&format!("{}-key", setting.key), &format!("{value}\n"));
        let file = toml(&format!("{}_file = \"{}\"\n", setting.key, secret.path()));
        let config = Config::load(&other_secrets(*setting), Some(&file)).unwrap();
        match *setting {
            DATABASE_URL => assert_eq!(config.database_url, *value),
            ENCRYPTION_KEYS => assert_eq!(config.encryption_keys.current_key_id(), "s1"),
            _ => panic!("no check for {}", setting.var),
        }
    }
}

#[test]
fn every_secret_set_twice_is_refused_by_name() {
    for (setting, value, hidden) in SECRET_SAMPLES {
        let mut vars = other_secrets(*setting);
        vars.insert(setting.var.to_owned(), (*value).to_owned());
        vars.insert(format!("{}_FILE", setting.var), "/run/secrets/x".to_owned());
        let error = Config::load(&vars, None).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("set only one of {} and {}_FILE", setting.var, setting.var)
        );

        let error = toml_error(&format!(
            "{} = \"{value}\"\n{}_file = \"/run/secrets/x\"\n",
            setting.key, setting.key
        ));
        assert_eq!(
            error,
            ConfigError::Twice {
                value: key_origin(setting.key, 1),
                file: key_origin(&format!("{}_file", setting.key), 2),
            }
        );
        assert!(!error.to_string().contains(hidden), "{error}");
    }
}

#[test]
fn an_unreadable_secret_file_names_the_variable_or_key_and_not_the_path() {
    for (setting, _, _) in SECRET_SAMPLES {
        let mut vars = other_secrets(*setting);
        vars.insert(
            format!("{}_FILE", setting.var),
            "/nonexistent/zq/secret".to_owned(),
        );
        let error = Config::load(&vars, None).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "{}_FILE names a file that could not be read: entity not found",
                setting.var
            )
        );

        let file = toml(&format!(
            "{}_file = \"/nonexistent/zq/secret\"\n",
            setting.key
        ));
        let error = Config::load(&other_secrets(*setting), Some(&file)).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "`{}_file` (/etc/stepclimb/stepclimb.toml line 1) names a file that could not be \
                 read: entity not found",
                setting.key
            )
        );
    }
}

#[test]
fn a_secret_pasted_into_a_file_variable_is_not_echoed() {
    let error = Config::load(
        &env(&[
            ("STEPCLIMB_DATABASE_URL", URL),
            ("STEPCLIMB_ENCRYPTION_KEYS_FILE", KEYS),
        ]),
        None,
    )
    .unwrap_err();
    let message = format!("{error} {error:?}");
    assert!(
        message.contains("STEPCLIMB_ENCRYPTION_KEYS_FILE"),
        "{message}"
    );
    assert!(!message.contains("AAEC"), "{message}");
}

#[test]
fn reads_the_encryption_key_ring_and_shows_only_its_ids() {
    let config = Config::load(
        &env(&[
            ("STEPCLIMB_DATABASE_URL", URL),
            ("STEPCLIMB_PUBLIC_URL", PUBLIC),
            (
                "STEPCLIMB_ENCRYPTION_KEYS",
                "k2:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=,k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            ),
        ]),
        None,
    )
    .unwrap();
    assert_eq!(config.encryption_keys.current_key_id(), "k2");
    let printed = format!("{config:?}");
    assert!(printed.contains(r#"key_ids: ["k2", "k1"]"#), "{printed}");
    assert!(
        !printed.contains("AAEC") && !printed.contains("ICEi"),
        "{printed}"
    );
}

#[test]
fn a_bad_key_is_refused_without_repeating_it() {
    let error = Config::load(
        &with_required(&[(
            "STEPCLIMB_ENCRYPTION_KEYS",
            "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYX",
        )]),
        None,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.starts_with(
            r#"STEPCLIMB_ENCRYPTION_KEYS: entry 1 (id "k1") is not 32 bytes in standard padded base64; for example k1:"#
        ),
        "{message}"
    );
    assert!(!message.contains("AAEC"), "{message}");
}

// T3: TOML errors name the file and line.

#[test]
fn a_toml_syntax_error_names_the_file_and_line_and_not_the_text() {
    let error =
        toml_error("bind = \"0.0.0.0:8080\"\ndatabase_url = \"postgres://u:zq-secret@h/db\n");
    let ConfigError::Toml { path, line, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(path, Path::new("/etc/stepclimb/stepclimb.toml"));
    assert_eq!(*line, Some(2));
    let message = format!("{error} {error:?}");
    assert!(
        message.starts_with("/etc/stepclimb/stepclimb.toml line 2: "),
        "{message}"
    );
    assert!(!message.contains("zq"), "{message}");
}

#[test]
fn an_unknown_key_names_the_file_and_line() {
    let error = toml_error("bind = \"0.0.0.0:8080\"\n\ndatabse_url = \"x\"\n");
    let message = error.to_string();
    assert!(
        message.starts_with("/etc/stepclimb/stepclimb.toml line 3: unknown field `databse_url`"),
        "{message}"
    );
}

#[test]
fn a_section_is_an_unknown_key() {
    let error = toml_error("[database]\nurl = \"postgres://u@h/db\"\n");
    let message = error.to_string();
    assert!(message.contains("unknown field `database`"), "{message}");
    assert!(message.contains(" line 1: "), "{message}");
}

#[test]
fn every_setting_key_is_accepted_in_the_file() {
    for setting in SETTINGS {
        let mut keys = vec![setting.key.to_owned()];
        if setting.secret {
            keys.push(format!("{}_file", setting.key));
        }
        for key in keys {
            let file = toml(&format!("{key} = \"x\"\n"));
            assert!(
                file.string(&key).unwrap().is_some(),
                "{key} is not read from FileKeys"
            );
        }
    }
}

#[test]
fn an_unreadable_config_file_names_its_path() {
    let error = ConfigFile::read(Path::new("/nonexistent/stepclimb.toml")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "could not read the configuration file /nonexistent/stepclimb.toml: entity not found"
    );
}

#[test]
fn reads_a_config_file_from_disk() {
    let on_disk = TempFile::new("stepclimb.toml", "log = \"warn\"\n");
    let file = ConfigFile::read(&on_disk.0).unwrap();
    let config = Config::load(&with_required(&[]), Some(&file)).unwrap();
    assert_eq!(config.log, "warn");
    assert_eq!(config.file.as_deref(), Some(on_disk.0.as_path()));
}

// T5: `Debug` hides every secret.

#[test]
fn debug_with_every_secret_set_shows_none_of_them() {
    let mut vars: Env = SECRET_SAMPLES
        .iter()
        .map(|(s, v, _)| (s.var.to_owned(), (*v).to_owned()))
        .collect();
    vars.insert(PUBLIC_URL.var.to_owned(), PUBLIC.to_owned());
    let config = Config::load(&vars, None).unwrap();
    let printed = format!("{config:?}");
    assert!(
        printed.contains(
            r#"database_url: "postgres://app@db.internal:5432/stepclimb (password and parameters hidden)""#
        ),
        "{printed}"
    );
    for (setting, _, hidden) in SECRET_SAMPLES {
        assert!(!printed.contains(hidden), "{}: {printed}", setting.var);
    }
}

#[test]
fn debug_hides_the_database_password_and_query() {
    let config = Config::load(
        &with_required(&[(
            "STEPCLIMB_DATABASE_URL",
            "postgres://app:s3cret-pa55@db.internal:5432/stepclimb?sslmode=require&password=q-s3cret",
        )]),
        None,
    )
    .unwrap();
    let printed = format!("{config:?}");
    assert!(!printed.contains("s3cret"), "{printed}");
}

#[test]
fn redaction_never_shows_a_password_with_odd_characters() {
    for url in [
        "postgres://u:zq%40ss%2Fw0rd@h/db",
        "postgres://u:zq?ss@h/db",
        "postgres://u:zq@ss@h/db",
        "postgres://u@h/db?password=zq",
        "postgres://u:123/zq@h/db",
        "postgres://u@h/db?dbname=zq.x",
        "not a url zq",
    ] {
        let printed = redacted_database_url(url);
        assert!(!printed.contains("zq"), "{url} -> {printed}");
    }
}

/// `stepclimb check-config` prints these; a new non-secret setting must be listed, and no secret
/// may be.
#[test]
fn non_secret_values_list_every_non_secret_setting_and_no_secret() {
    let mut pairs: Vec<(&str, &str)> = vec![
        ("STEPCLIMB_BIND", "0.0.0.0:9000"),
        ("STEPCLIMB_PUBLIC_URL", PUBLIC),
    ];
    pairs.extend(SECRET_SAMPLES.iter().map(|(s, value, _)| (s.var, *value)));
    let config = Config::load(&env(&pairs), None).unwrap();
    let values = config.non_secret_values();
    let keys: Vec<&str> = values.iter().map(|(key, _)| *key).collect();
    let expected: Vec<&str> = SETTINGS
        .iter()
        .filter(|s| !s.secret)
        .map(|s| s.key)
        .collect();
    assert_eq!(keys, expected);
    assert_eq!(
        values,
        [
            ("bind", "0.0.0.0:9000".to_owned()),
            ("log", "info".to_owned()),
            ("public_url", PUBLIC.to_owned()),
            ("allowed_origins", "(none)".to_owned()),
            ("trusted_proxies", "(none)".to_owned()),
            ("sign_in_rate_limit", "20".to_owned()),
            ("token_rate_limit", "60".to_owned()),
            ("failed_sign_in_limit", "10".to_owned()),
            ("allow_insecure_providers", "false".to_owned()),
        ]
    );
    let printed = format!("{values:?}");
    for (setting, _, hidden) in SECRET_SAMPLES {
        assert!(!printed.contains(hidden), "{}: {printed}", setting.var);
    }
}

/// Everything `work` logs at any level, as text.
fn logged(work: impl FnOnce()) -> String {
    let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || Writer(writer.clone()))
        .finish();
    tracing::subscriber::with_default(subscriber, work);
    String::from_utf8(buffer.lock().unwrap().clone()).unwrap()
}

struct Writer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_database_url_parameter_sqlx_ignores_is_refused_by_position_only() {
    let output = logged(|| {
        let error = Config::load(
            &with_required(&[(
                "STEPCLIMB_DATABASE_URL",
                "postgres://u@h/db?sslmode=require&sslpassword=zqsecret",
            )]),
            None,
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.starts_with(
                "STEPCLIMB_DATABASE_URL: query parameter 2 of the URL is not one the server reads"
            ),
            "{message}"
        );
        assert!(!message.contains("zqsecret"), "{message}");
        assert!(!message.contains("sslpassword"), "{message}");
    });
    assert!(!output.contains("zqsecret"), "{output}");
}

#[test]
fn an_unparsable_database_url_is_refused_without_repeating_it() {
    for url in [
        "postgres://u:zq@h:notaport/db",
        "postgres://u@h/db?sslmode=zq",
        "postgresql://u@h/db?port=zq",
    ] {
        let error =
            Config::load(&with_required(&[("STEPCLIMB_DATABASE_URL", url)]), None).unwrap_err();
        let message = error.to_string();
        assert!(
            message
                .starts_with("STEPCLIMB_DATABASE_URL: is not a valid PostgreSQL URL; for example"),
            "{url}: {message}"
        );
        assert!(!message.contains("zq"), "{url}: {message}");
    }
}

/// Every parameter the loader accepts is one sqlx reads without its "unrecognized" warning, so a
/// sqlx upgrade that drops one fails here.
#[test]
fn database_url_parameters_sqlx_accepts() {
    let url = "postgres://u@h/db?sslmode=require&ssl-mode=prefer&sslrootcert=/r&ssl-root-cert=/r\
               &ssl-ca=/r&sslcert=/c&ssl-cert=/c&sslkey=/k&ssl-key=/k&statement-cache-capacity=10\
               &host=h&hostaddr=127.0.0.1&port=5432&dbname=d&user=u&password=p\
               &application_name=a&options=-c%20a%3Db&options[search_path]=s";
    let named: Vec<String> = url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(name, _)| name.into_owned())
        .filter(|name| !name.starts_with("options["))
        .collect();
    assert_eq!(
        named, DATABASE_URL_PARAMETERS,
        "the URL above names every parameter"
    );
    let output = logged(|| {
        Config::load(&with_required(&[("STEPCLIMB_DATABASE_URL", url)]), None).unwrap();
        url.parse::<PgConnectOptions>().unwrap();
    });
    assert!(!output.contains("unrecognized"), "{output}");
    let unknown = logged(|| {
        "postgres://u@h/db?sslpassword=x"
            .parse::<PgConnectOptions>()
            .unwrap();
    });
    assert!(
        unknown.contains("unrecognized"),
        "sqlx no longer warns: {unknown}"
    );
}

#[test]
fn redacting_a_url_with_an_unknown_parameter_logs_nothing() {
    let mut printed = String::new();
    let output = logged(|| {
        printed = redacted_database_url("postgres://u@h/db?sslpassword=zqsecret");
    });
    assert_eq!(output, "");
    assert_eq!(printed, "<not a parseable PostgreSQL URL>");
}

#[test]
fn an_unencoded_ampersand_in_a_password_is_refused_without_its_tail() {
    let error = Config::load(
        &with_required(&[("STEPCLIMB_DATABASE_URL", "postgres://u@h/db?password=ab&zq")]),
        None,
    )
    .unwrap_err();
    let message = error.to_string();
    assert!(
        message.starts_with(
            "STEPCLIMB_DATABASE_URL: query parameter 2 of the URL is not one the server reads"
        ),
        "{message}"
    );
    assert!(!message.contains("zq"), "{message}");
}

#[test]
fn an_at_sign_after_the_host_is_refused_without_repeating_the_url() {
    for url in ["postgres://u:123/zq@h/db", "postgres://u@h/zq@db"] {
        let error =
            Config::load(&with_required(&[("STEPCLIMB_DATABASE_URL", url)]), None).unwrap_err();
        let message = error.to_string();
        assert!(
            message.starts_with("STEPCLIMB_DATABASE_URL: has an @ after the host;"),
            "{url}: {message}"
        );
        assert!(!message.contains("zq"), "{url}: {message}");
    }
}

// Spec 08 Q1, Q6: the public URL and the allowed origins.

#[test]
fn a_missing_public_url_is_refused_by_name_with_an_example() {
    let error = Config::load(
        &env(&[
            ("STEPCLIMB_DATABASE_URL", URL),
            ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
        ]),
        None,
    )
    .unwrap_err();
    assert_eq!(error, ConfigError::Missing(PUBLIC_URL));
    assert!(
        error
            .to_string()
            .contains("for example https://lms.example.org"),
        "{error}"
    );
}

#[test]
fn the_public_url_is_kept_as_its_origin() {
    let config = Config::load(
        &with_required(&[("STEPCLIMB_PUBLIC_URL", "https://LMS.example.org:443/")]),
        None,
    )
    .unwrap();
    assert_eq!(config.public_url.as_str(), "https://lms.example.org");
    assert!(config.public_url.is_https());
    assert!(config.allowed_origins.is_empty());
}

#[test]
fn a_public_url_with_a_path_is_refused_naming_the_setting() {
    let error = Config::load(
        &with_required(&[("STEPCLIMB_PUBLIC_URL", "https://example.org/lms")]),
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "STEPCLIMB_PUBLIC_URL: must be an origin only, with no path, query or fragment; for \
         example https://lms.example.org"
    );
}

#[test]
fn allowed_origins_are_a_comma_separated_list() {
    let config = Config::load(
        &with_required(&[(
            "STEPCLIMB_ALLOWED_ORIGINS",
            "https://app.example.org, http://localhost:5173/",
        )]),
        None,
    )
    .unwrap();
    let origins: Vec<&str> = config
        .allowed_origins
        .iter()
        .map(WebOrigin::as_str)
        .collect();
    assert_eq!(
        origins,
        ["https://app.example.org", "http://localhost:5173"]
    );
    assert_eq!(
        config.allowed_origins_list(),
        "https://app.example.org,http://localhost:5173"
    );
}

#[test]
fn a_bad_allowed_origin_is_refused_by_position() {
    for (value, problem) in [
        (
            "https://app.example.org,ftp://files.example.org",
            "entry 2 must start with http:// or https://",
        ),
        ("https://app.example.org,", "entry 2 is not a URL"),
    ] {
        let error = Config::load(
            &with_required(&[("STEPCLIMB_ALLOWED_ORIGINS", value)]),
            None,
        )
        .unwrap_err();
        assert!(
            error.to_string().starts_with(&format!(
                "STEPCLIMB_ALLOWED_ORIGINS: {problem}; for example "
            )),
            "{error}"
        );
    }
}

#[test]
fn the_public_url_and_allowed_origins_are_read_from_the_file() {
    let file = toml(
        "public_url = \"https://lms.example.org\"\nallowed_origins = \"https://app.example.org\"\n",
    );
    let vars = env(&[
        ("STEPCLIMB_DATABASE_URL", URL),
        ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
    ]);
    let config = Config::load(&vars, Some(&file)).unwrap();
    assert_eq!(config.public_url.as_str(), "https://lms.example.org");
    assert_eq!(config.allowed_origins_list(), "https://app.example.org");
}

/// An origin with credentials in it is refused without printing the password, in the
/// environment or the file, in either setting, and every other refusal leaves the value out too.
#[test]
fn an_origin_setting_never_repeats_its_value() {
    for value in [
        "https://admin:hunter2@lms.example.org",
        "https://hunter2.example.org/path",
        "ftp://hunter2.example.org",
        "hunter2",
    ] {
        let list = format!("https://app.example.org,{value}");
        let errors = [
            Config::load(&with_required(&[("STEPCLIMB_PUBLIC_URL", value)]), None).unwrap_err(),
            Config::load(
                &with_required(&[("STEPCLIMB_ALLOWED_ORIGINS", &list)]),
                None,
            )
            .unwrap_err(),
            Config::load(
                &env(&[
                    ("STEPCLIMB_DATABASE_URL", URL),
                    ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
                ]),
                Some(&toml(&format!("public_url = \"{value}\"\n"))),
            )
            .unwrap_err(),
        ];
        for error in errors {
            let printed = format!("{error} {error:?}");
            assert!(!printed.contains("hunter2"), "{value}: {printed}");
        }
    }
}

// Spec 08 Q7: trusted proxies and the rate-limit numbers.

#[test]
fn no_proxy_is_trusted_and_q7s_numbers_apply_when_unset() {
    let config = Config::load(&with_required(&[]), None).unwrap();
    assert!(config.trusted_proxies.is_empty());
    assert_eq!(config.limits(), crate::rate_limit::Limits::DEFAULT);
}

#[test]
fn trusted_proxies_are_addresses_and_ranges() {
    let config = Config::load(
        &with_required(&[("STEPCLIMB_TRUSTED_PROXIES", "10.201.73.2, fd00::/32")]),
        None,
    )
    .unwrap();
    assert_eq!(config.trusted_proxies_or_none(), "10.201.73.2,fd00::/32");
    let peer: std::net::IpAddr = "10.201.73.2".parse().unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-forwarded-for", "203.0.113.9".parse().unwrap());
    assert_eq!(
        config.trusted_proxies.client_ip(peer, &headers).to_string(),
        "203.0.113.9"
    );
}

#[test]
fn a_bad_trusted_proxy_is_refused_by_position() {
    let error = Config::load(
        &with_required(&[("STEPCLIMB_TRUSTED_PROXIES", "10.201.73.2,caddy")]),
        None,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!(
            "STEPCLIMB_TRUSTED_PROXIES: entry 2 is not an IP address or CIDR range; for example {}",
            TRUSTED_PROXIES.example
        )
    );
}

#[test]
fn the_limits_are_configurable() {
    let config = Config::load(
        &with_required(&[
            ("STEPCLIMB_SIGN_IN_RATE_LIMIT", "5"),
            ("STEPCLIMB_TOKEN_RATE_LIMIT", "120"),
            ("STEPCLIMB_FAILED_SIGN_IN_LIMIT", "3"),
        ]),
        None,
    )
    .unwrap();
    let limits = config.limits();
    assert_eq!(
        (
            limits.sign_in.get(),
            limits.token.get(),
            limits.failed_checks.get()
        ),
        (5, 120, 3)
    );
}

#[test]
fn a_limit_must_be_a_whole_number_of_one_or_more() {
    for setting in [SIGN_IN_RATE_LIMIT, TOKEN_RATE_LIMIT, FAILED_SIGN_IN_LIMIT] {
        for value in ["0", "-1", "+5", " 5", "ten", "1.5", "4294967296"] {
            let error = Config::load(&with_required(&[(setting.var, value)]), None).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "{}: {value:?} is not a whole number of 1 or more; for example {}",
                    setting.var, setting.example
                ),
                "{value}"
            );
        }
    }
}

#[test]
fn the_proxies_and_limits_are_read_from_the_file() {
    let file = toml(
        "public_url = \"https://lms.example.org\"\ntrusted_proxies = \"10.201.73.2\"\n\
         sign_in_rate_limit = \"7\"\ntoken_rate_limit = \"8\"\nfailed_sign_in_limit = \"9\"\n",
    );
    let vars = env(&[
        ("STEPCLIMB_DATABASE_URL", URL),
        ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
    ]);
    let config = Config::load(&vars, Some(&file)).unwrap();
    assert_eq!(config.trusted_proxies_or_none(), "10.201.73.2");
    let limits = config.limits();
    assert_eq!(
        (
            limits.sign_in.get(),
            limits.token.get(),
            limits.failed_checks.get()
        ),
        (7, 8, 9)
    );
}

#[test]
fn a_trusted_range_wider_than_slash_8_is_refused_naming_the_setting() {
    for (value, position) in [("0.0.0.0/0", 1), ("10.201.73.2,::/0", 2)] {
        let error = Config::load(
            &with_required(&[("STEPCLIMB_TRUSTED_PROXIES", value)]),
            None,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "STEPCLIMB_TRUSTED_PROXIES: entry {position} is wider than /8 (IPv4) or /32 \
                 (IPv6); a trusted range must hold proxies only, never clients; for example {}",
                TRUSTED_PROXIES.example
            ),
            "{value}"
        );
    }
}

/// The configuration reference in the self-hosting guide: one row per variable, its cells
/// trimmed (`docs/specs/m0/14-compose-deploy.md` B12, T4).
fn reference_rows() -> Vec<Vec<String>> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/compose/README.md"
    );
    let guide = std::fs::read_to_string(path).expect("deploy/compose/README.md is readable");
    guide
        .lines()
        .filter(|line| line.starts_with("| `STEPCLIMB_"))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_owned())
                .collect()
        })
        .collect()
}

/// The Default cell the reference gives `setting`: `required` when the loader refuses its
/// absence, `none` when its unset value is empty, otherwise the value the loader uses.
fn reference_default(setting: &Setting) -> String {
    let all = with_required(&[]);
    let mut without = all.clone();
    without.remove(setting.var);
    match Config::load(&without, None) {
        Err(ConfigError::Missing(missing)) if missing == *setting => return "required".to_owned(),
        Err(error) => panic!("{} unset: {error}", setting.var),
        Ok(_) => {}
    }
    let config = Config::load(&all, None).unwrap();
    let (_, value) = config
        .non_secret_values()
        .into_iter()
        .find(|(key, _)| *key == setting.key)
        .unwrap_or_else(|| {
            panic!(
                "{} has a default but is not a non-secret value",
                setting.var
            )
        });
    if value == "(none)" {
        "none".to_owned()
    } else {
        format!("`{value}`")
    }
}

#[test]
fn every_setting_is_in_the_configuration_reference() {
    let rows = reference_rows();
    for setting in SETTINGS {
        let row = rows
            .iter()
            .find(|row| row[0].starts_with(&format!("`{}`", setting.var)))
            .unwrap_or_else(|| {
                panic!(
                    "{} is missing from the configuration reference in deploy/compose/README.md",
                    setting.var
                )
            });
        let (var, key) = (setting.var, setting.key);
        let (vars, keys) = if setting.secret {
            (
                format!("`{var}`, `{var}_FILE`"),
                format!("`{key}`, `{key}_file`"),
            )
        } else {
            (format!("`{var}`"), format!("`{key}`"))
        };
        assert_eq!(
            row.len(),
            6,
            "{var}: Variable, TOML key, Type, Default, Secret, Meaning"
        );
        assert_eq!((&row[0], &row[1]), (&vars, &keys), "{var}");
        assert_eq!(row[3], reference_default(setting), "{var}: Default");
        let secret = if setting.secret { "yes" } else { "no" };
        assert_eq!(row[4], secret, "{var}: Secret");
    }
}

#[test]
fn the_configuration_reference_names_only_variables_the_server_reads() {
    let rows = reference_rows();
    for row in &rows {
        let var = row[0].split('`').nth(1).unwrap_or_default();
        assert!(
            known_vars().iter().any(|known| known == var),
            "{var} is not read"
        );
    }
    let config = rows.iter().find(|row| row[0] == format!("`{CONFIG_VAR}`"));
    assert!(
        config.is_some(),
        "{CONFIG_VAR} is missing from the reference"
    );
    assert_eq!(
        rows.len(),
        SETTINGS.len() + 1,
        "one row per setting and {CONFIG_VAR}"
    );
}

#[test]
fn the_insecure_providers_switch_is_off_unless_set_true() {
    let unset = Config::load(&with_required(&[]), None).unwrap();
    assert!(!unset.allow_insecure_providers);
    for (value, on) in [("true", true), ("false", false), ("", false)] {
        let config = Config::load(
            &with_required(&[(ALLOW_INSECURE_PROVIDERS.var, value)]),
            None,
        )
        .unwrap();
        assert_eq!(config.allow_insecure_providers, on, "{value:?}");
    }
    for value in ["yes", "1", "TRUE", " true"] {
        let error = Config::load(
            &with_required(&[(ALLOW_INSECURE_PROVIDERS.var, value)]),
            None,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "STEPCLIMB_ALLOW_INSECURE_PROVIDERS: {value:?} is not true or false; for example {}",
                ALLOW_INSECURE_PROVIDERS.example
            ),
            "{value:?}"
        );
    }
}

#[test]
fn the_insecure_providers_switch_is_read_from_the_file() {
    let file =
        toml("public_url = \"https://lms.example.org\"\nallow_insecure_providers = \"true\"\n");
    let vars = env(&[
        ("STEPCLIMB_DATABASE_URL", URL),
        ("STEPCLIMB_ENCRYPTION_KEYS", KEYS),
    ]);
    assert!(
        Config::load(&vars, Some(&file))
            .unwrap()
            .allow_insecure_providers
    );
}
