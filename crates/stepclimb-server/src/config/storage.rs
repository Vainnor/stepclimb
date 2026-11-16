// SPDX-License-Identifier: AGPL-3.0-or-later
//! The object storage settings (docs/specs/m1/06-files.md B1, B1a, Q1): where course files are
//! stored, checked without connecting to anything.
//!
//! `storage_url` is optional (Q1): unset, storage is [`StorageConfig::NotConfigured`] and every
//! route that reads or writes stored bytes answers 503 `storage.not_configured`. Set, it is
//! `file:///<absolute path>` (a local directory) or `s3://<bucket>[/<prefix>]` (S3 or an
//! S3-compatible store such as MinIO). The `storage_s3_*` settings apply only to an `s3://` URL:
//! with no URL or a `file://` one, any of them set is refused naming it, so a half-configured
//! store cannot start as "not configured". An `s3://` URL needs both access key settings: the
//! server reads no credentials from the environment or an instance profile.

use std::fmt;
use std::path::PathBuf;

use super::{ConfigError, Origin, Setting, Sources};

pub const STORAGE_URL: Setting = Setting {
    var: "STEPCLIMB_STORAGE_URL",
    key: "storage_url",
    secret: false,
    example: "s3://stepclimb or file:///var/lib/stepclimb/storage",
};
pub const STORAGE_S3_ENDPOINT: Setting = Setting {
    var: "STEPCLIMB_STORAGE_S3_ENDPOINT",
    key: "storage_s3_endpoint",
    secret: false,
    example: "http://minio:9000 (unset for AWS S3)",
};
pub const STORAGE_S3_REGION: Setting = Setting {
    var: "STEPCLIMB_STORAGE_S3_REGION",
    key: "storage_s3_region",
    secret: false,
    example: "us-east-1",
};
pub const STORAGE_S3_ACCESS_KEY_ID: Setting = Setting {
    var: "STEPCLIMB_STORAGE_S3_ACCESS_KEY_ID",
    key: "storage_s3_access_key_id",
    secret: false,
    example: "stepclimb (the access key the store issued)",
};
pub const STORAGE_S3_SECRET_ACCESS_KEY: Setting = Setting {
    var: "STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY",
    key: "storage_s3_secret_access_key",
    secret: true,
    example: "the secret the store issued with the access key",
};
pub const STORAGE_S3_ALLOW_HTTP: Setting = Setting {
    var: "STEPCLIMB_STORAGE_S3_ALLOW_HTTP",
    key: "storage_s3_allow_http",
    secret: false,
    example: "true (only for a store on the same host or private network) or false",
};

/// The region when `storage_s3_region` is unset.
pub const DEFAULT_REGION: &str = "us-east-1";

/// The S3 settings, in the order `check-config` prints them.
const S3_SETTINGS: [Setting; 5] = [
    STORAGE_S3_ENDPOINT,
    STORAGE_S3_REGION,
    STORAGE_S3_ACCESS_KEY_ID,
    STORAGE_S3_SECRET_ACCESS_KEY,
    STORAGE_S3_ALLOW_HTTP,
];

/// Where course files are stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageConfig {
    /// `storage_url` is unset (B1a).
    NotConfigured,
    /// `file:///<root>`: a local directory, which must exist when `serve` starts.
    Local { root: PathBuf },
    /// `s3://<bucket>[/<prefix>]`.
    S3(S3Config),
}

/// An S3 or S3-compatible store. `Debug` hides the secret access key.
#[derive(Clone, PartialEq, Eq)]
pub struct S3Config {
    pub bucket: String,
    /// The key prefix inside the bucket, without leading or trailing `/`; empty for none.
    pub prefix: String,
    /// The store's origin (`http://minio:9000`); `None` for AWS S3.
    pub endpoint: Option<String>,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// Whether the store may be reached over plain `http`.
    pub allow_http: bool,
}

impl fmt::Debug for S3Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Config")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"(hidden)")
            .field("allow_http", &self.allow_http)
            .finish()
    }
}

impl StorageConfig {
    /// The `storage_url` this was read from, as `check-config` and the startup log show it.
    #[must_use]
    pub fn url(&self) -> Option<String> {
        match self {
            Self::NotConfigured => None,
            Self::Local { root } => Some(format!("file://{}", root.display())),
            Self::S3(s3) if s3.prefix.is_empty() => Some(format!("s3://{}", s3.bucket)),
            Self::S3(s3) => Some(format!("s3://{}/{}", s3.bucket, s3.prefix)),
        }
    }

    /// The lines `stepclimb check-config` prints for storage, as key and value. The secret
    /// access key is shown only as set.
    #[must_use]
    pub fn report(&self) -> Vec<(&'static str, String)> {
        let url = || (STORAGE_URL.key, self.url().unwrap_or_default());
        match self {
            Self::NotConfigured => {
                vec![("storage", format!("not configured ({})", STORAGE_URL.var))]
            }
            Self::Local { .. } => vec![url()],
            Self::S3(s3) => vec![
                url(),
                (
                    STORAGE_S3_ENDPOINT.key,
                    s3.endpoint.clone().unwrap_or_else(|| "(AWS S3)".to_owned()),
                ),
                (STORAGE_S3_REGION.key, s3.region.clone()),
                (STORAGE_S3_ACCESS_KEY_ID.key, s3.access_key_id.clone()),
                (STORAGE_S3_SECRET_ACCESS_KEY.key, "(set, hidden)".to_owned()),
                (STORAGE_S3_ALLOW_HTTP.key, s3.allow_http.to_string()),
            ],
        }
    }
}

impl fmt::Display for StorageConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.url() {
            Some(url) => f.write_str(&url),
            None => write!(f, "not configured ({})", STORAGE_URL.var),
        }
    }
}

/// Read and check the storage settings.
pub(super) fn load(sources: &Sources<'_>) -> Result<StorageConfig, ConfigError> {
    let Some((url, origin)) = sources.plain(&STORAGE_URL)? else {
        refuse_s3_settings(sources, "STEPCLIMB_STORAGE_URL is not set")?;
        return Ok(StorageConfig::NotConfigured);
    };
    let invalid = |problem: String| ConfigError::Invalid {
        origin: origin.clone(),
        problem: format!("{problem}; for example {}", STORAGE_URL.example),
    };
    let parsed = url::Url::parse(&url).map_err(|_| invalid("is not a URL".to_owned()))?;
    match parsed.scheme() {
        "file" => {
            refuse_s3_settings(sources, "STEPCLIMB_STORAGE_URL is a file:// URL")?;
            local_root(&parsed).map_err(invalid)
        }
        "s3" => {
            let (bucket, prefix) = bucket_and_prefix(&parsed).map_err(invalid)?;
            s3(sources, bucket, prefix)
        }
        _ => Err(invalid("must start with file:// or s3://".to_owned())),
    }
}

/// Refuse the first S3 setting that is set, naming it and `why` it does not apply.
fn refuse_s3_settings(sources: &Sources<'_>, why: &str) -> Result<(), ConfigError> {
    for setting in S3_SETTINGS {
        if let Some(origin) = sources.origin_if_set(&setting)? {
            return Err(ConfigError::Invalid {
                origin,
                problem: format!(
                    "applies only to an s3:// storage URL, and {why}; set {} to s3://<bucket> \
                     or remove this setting",
                    STORAGE_URL.var
                ),
            });
        }
    }
    Ok(())
}

/// The absolute directory of a `file://` URL, with no host, query or fragment.
fn local_root(url: &url::Url) -> Result<StorageConfig, String> {
    if url.host_str().is_some_and(|host| !host.is_empty()) {
        return Err("a file:// URL names no host: write file:///<absolute path>".to_owned());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("has a query or fragment".to_owned());
    }
    let root = url
        .to_file_path()
        .map_err(|()| "does not name an absolute directory".to_owned())?;
    if root.parent().is_none() {
        return Err("names the root directory; name a directory for Stepclimb's files".to_owned());
    }
    Ok(StorageConfig::Local { root })
}

/// The bucket and prefix of an `s3://` URL. The bucket follows S3's naming rules; each prefix
/// segment is 1 to 100 of `A-Z a-z 0-9 . _ -` and not `.` or `..`. No credentials, port, query
/// or fragment.
fn bucket_and_prefix(url: &url::Url) -> Result<(String, String), String> {
    if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return Err("an s3:// URL holds only the bucket and a prefix".to_owned());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("has a query or fragment".to_owned());
    }
    let bucket = url.host_str().unwrap_or_default();
    if !is_bucket_name(bucket) {
        return Err(
            "the bucket is 3 to 63 lowercase letters, digits, dots and hyphens, starting and \
             ending with a letter or digit"
                .to_owned(),
        );
    }
    let prefix = url.path().trim_matches('/');
    if !prefix.is_empty() && !prefix.split('/').all(is_prefix_segment) {
        return Err(
            "each prefix segment is 1 to 100 of A-Z a-z 0-9 . _ - and not . or ..".to_owned(),
        );
    }
    Ok((bucket.to_owned(), prefix.to_owned()))
}

fn is_bucket_name(name: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    (3..=63).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && edge(name.chars().next())
        && edge(name.chars().last())
}

fn is_prefix_segment(segment: &str) -> bool {
    (1..=100).contains(&segment.len())
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The S3 settings for `bucket` and `prefix`: both access key settings are required.
fn s3(sources: &Sources<'_>, bucket: String, prefix: String) -> Result<StorageConfig, ConfigError> {
    let allow_http = sources.flag(&STORAGE_S3_ALLOW_HTTP)?;
    let endpoint = match sources.plain(&STORAGE_S3_ENDPOINT)? {
        Some((value, origin)) => Some(endpoint(&value, origin, allow_http)?),
        None => None,
    };
    let region = match sources.plain(&STORAGE_S3_REGION)? {
        Some((value, origin)) => region(value, origin)?,
        None => DEFAULT_REGION.to_owned(),
    };
    let access_key_id = sources
        .plain(&STORAGE_S3_ACCESS_KEY_ID)?
        .map(|(value, _)| value)
        .ok_or(ConfigError::Missing(STORAGE_S3_ACCESS_KEY_ID))?;
    let (secret_access_key, _) = sources.secret(&STORAGE_S3_SECRET_ACCESS_KEY)?;
    Ok(StorageConfig::S3(S3Config {
        bucket,
        prefix,
        endpoint,
        region,
        access_key_id,
        secret_access_key,
        allow_http,
    }))
}

/// An `http` or `https` origin without credentials, path, query or fragment; `http` only with
/// `storage_s3_allow_http`. Errors never repeat the value, which could carry credentials.
fn endpoint(value: &str, origin: Origin, allow_http: bool) -> Result<String, ConfigError> {
    let invalid = |problem: &str| ConfigError::Invalid {
        origin: origin.clone(),
        problem: format!("{problem}; for example {}", STORAGE_S3_ENDPOINT.example),
    };
    let url = url::Url::parse(value).map_err(|_| invalid("is not a URL"))?;
    let plain = url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.host_str().is_some();
    if !plain {
        return Err(invalid(
            "is an origin only: a scheme, a host and an optional port",
        ));
    }
    match url.scheme() {
        "https" => {}
        "http" if allow_http => {}
        "http" => {
            return Err(invalid(&format!(
                "is http, which needs {} set to true",
                STORAGE_S3_ALLOW_HTTP.var
            )));
        }
        _ => return Err(invalid("must start with http:// or https://")),
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// 1 to 64 of `a-z 0-9 -`, as every S3 region name is.
fn region(value: String, origin: Origin) -> Result<String, ConfigError> {
    let valid = (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(value)
    } else {
        Err(ConfigError::Invalid {
            origin,
            problem: format!(
                "{value:?} is not a region name; for example {}",
                STORAGE_S3_REGION.example
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::{Config, ConfigError, ConfigFile, Env, Origin};
    use super::*;

    const SECRET: &str = "zq-s3-secret";

    fn load(pairs: &[(&str, &str)]) -> Result<StorageConfig, ConfigError> {
        let mut env: Env = [
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            (
                "STEPCLIMB_ENCRYPTION_KEYS",
                "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            ),
            ("STEPCLIMB_PUBLIC_URL", "https://lms.example.org"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        env.extend(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        Config::load(&env, None).map(|config| config.storage)
    }

    /// An `s3://` URL with both keys, plus `pairs`.
    fn s3(pairs: &[(&str, &str)]) -> Result<StorageConfig, ConfigError> {
        let mut all = vec![
            ("STEPCLIMB_STORAGE_S3_ACCESS_KEY_ID", "test-key"),
            ("STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY", SECRET),
        ];
        if !pairs.iter().any(|(k, _)| *k == STORAGE_URL.var) {
            all.push((STORAGE_URL.var, "s3://stepclimb"));
        }
        all.extend_from_slice(pairs);
        load(&all)
    }

    fn s3_config(pairs: &[(&str, &str)]) -> S3Config {
        match s3(pairs).unwrap() {
            StorageConfig::S3(s3) => s3,
            other => panic!("not S3: {other:?}"),
        }
    }

    fn refused_at(error: &ConfigError) -> String {
        match error {
            ConfigError::Invalid { origin, .. } => origin.to_string(),
            other => panic!("not Invalid: {other}"),
        }
    }

    #[test]
    fn without_a_url_storage_is_not_configured_and_says_which_variable_to_set() {
        let storage = load(&[]).unwrap();
        assert_eq!(storage, StorageConfig::NotConfigured);
        assert_eq!(
            storage.report(),
            [(
                "storage",
                "not configured (STEPCLIMB_STORAGE_URL)".to_owned()
            )]
        );
        assert_eq!(
            storage.to_string(),
            "not configured (STEPCLIMB_STORAGE_URL)"
        );
    }

    #[test]
    fn an_s3_setting_without_a_url_is_refused_by_name() {
        for (var, value) in [
            ("STEPCLIMB_STORAGE_S3_ENDPOINT", "https://s3.example.org"),
            ("STEPCLIMB_STORAGE_S3_REGION", "eu-west-1"),
            ("STEPCLIMB_STORAGE_S3_ACCESS_KEY_ID", "key"),
            ("STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY", SECRET),
            (
                "STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY_FILE",
                "/run/secrets/s3",
            ),
            ("STEPCLIMB_STORAGE_S3_ALLOW_HTTP", "false"),
        ] {
            let error = load(&[(var, value)]).unwrap_err();
            assert_eq!(refused_at(&error), var);
            let message = error.to_string();
            assert!(
                message.contains("STEPCLIMB_STORAGE_URL is not set"),
                "{message}"
            );
            assert!(!message.contains(SECRET), "{message}");
        }
    }

    #[test]
    fn an_s3_key_in_the_file_without_a_url_is_refused_by_key_and_line() {
        let file = ConfigFile::parse(
            PathBuf::from("/etc/stepclimb/stepclimb.toml"),
            "bind = \"0.0.0.0:8080\"\nstorage_s3_secret_access_key_file = \"s3\"\n".to_owned(),
        )
        .unwrap();
        let env: Env = [
            ("STEPCLIMB_DATABASE_URL", "postgres://u@h/db"),
            (
                "STEPCLIMB_ENCRYPTION_KEYS",
                "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
            ),
            ("STEPCLIMB_PUBLIC_URL", "https://lms.example.org"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let error = Config::load(&env, Some(&file)).unwrap_err();
        assert!(
            matches!(
                &error,
                ConfigError::Invalid { origin: Origin::Key { key, line: 2, .. }, .. }
                    if key == "storage_s3_secret_access_key_file"
            ),
            "{error}"
        );
    }

    #[test]
    fn a_file_url_names_an_absolute_directory() {
        assert_eq!(
            load(&[(STORAGE_URL.var, "file:///var/lib/stepclimb/storage")]).unwrap(),
            StorageConfig::Local {
                root: PathBuf::from("/var/lib/stepclimb/storage")
            }
        );
        let storage = load(&[(STORAGE_URL.var, "file:///srv/files/")]).unwrap();
        assert_eq!(
            storage.report(),
            [("storage_url", "file:///srv/files/".to_owned())]
        );
        for bad in [
            "file://host/srv/files",
            "file:///",
            "file:///srv?x=1",
            "file:///srv#top",
        ] {
            let error = load(&[(STORAGE_URL.var, bad)]).unwrap_err();
            assert_eq!(refused_at(&error), STORAGE_URL.var, "{bad}");
        }
    }

    #[test]
    fn a_file_url_with_an_s3_setting_is_refused() {
        let error = load(&[
            (STORAGE_URL.var, "file:///srv/files"),
            ("STEPCLIMB_STORAGE_S3_REGION", "eu-west-1"),
        ])
        .unwrap_err();
        assert_eq!(refused_at(&error), "STEPCLIMB_STORAGE_S3_REGION");
        assert!(
            error
                .to_string()
                .contains("STEPCLIMB_STORAGE_URL is a file:// URL"),
            "{error}"
        );
    }

    #[test]
    fn an_s3_url_names_a_bucket_and_a_prefix() {
        let plain = s3_config(&[]);
        assert_eq!(
            (plain.bucket.as_str(), plain.prefix.as_str()),
            ("stepclimb", "")
        );
        let prefixed = s3_config(&[(STORAGE_URL.var, "s3://my.bucket-1/lms/files_v1/")]);
        assert_eq!(prefixed.bucket, "my.bucket-1");
        assert_eq!(prefixed.prefix, "lms/files_v1");
        assert_eq!(prefixed.region, DEFAULT_REGION);
        assert_eq!(prefixed.endpoint, None);
        assert!(!prefixed.allow_http);
    }

    #[test]
    fn a_bad_storage_url_is_refused_by_name_without_its_credentials() {
        for bad in [
            "not a url",
            "https://stepclimb",
            "s3://AB",
            "s3://ab",
            "s3://-bucket",
            "s3://bucket-",
            "s3://under_score",
            &format!("s3://{}", "a".repeat(64)),
            "s3://zq-user:zq-pass@bucket",
            "s3://bucket:9000",
            "s3://bucket/a?x=1",
            "s3://bucket/a#b",
            "s3://bucket/a//b",
            "s3://bucket/sp ace",
        ] {
            let error = s3(&[(STORAGE_URL.var, bad)]).unwrap_err();
            assert_eq!(refused_at(&error), STORAGE_URL.var, "{bad}");
            assert!(!error.to_string().contains("zq-pass"), "{error}");
        }
    }

    #[test]
    fn an_s3_url_needs_both_access_keys() {
        let error = load(&[
            (STORAGE_URL.var, "s3://stepclimb"),
            ("STEPCLIMB_STORAGE_S3_SECRET_ACCESS_KEY", SECRET),
        ])
        .unwrap_err();
        assert_eq!(error, ConfigError::Missing(STORAGE_S3_ACCESS_KEY_ID));
        let error = load(&[
            (STORAGE_URL.var, "s3://stepclimb"),
            ("STEPCLIMB_STORAGE_S3_ACCESS_KEY_ID", "key"),
        ])
        .unwrap_err();
        assert_eq!(error, ConfigError::Missing(STORAGE_S3_SECRET_ACCESS_KEY));
    }

    #[test]
    fn an_http_endpoint_needs_allow_http() {
        let error = s3(&[("STEPCLIMB_STORAGE_S3_ENDPOINT", "http://minio:9000")]).unwrap_err();
        assert_eq!(refused_at(&error), "STEPCLIMB_STORAGE_S3_ENDPOINT");
        assert!(
            error
                .to_string()
                .contains("STEPCLIMB_STORAGE_S3_ALLOW_HTTP"),
            "{error}"
        );
        let config = s3_config(&[
            ("STEPCLIMB_STORAGE_S3_ENDPOINT", "http://minio:9000/"),
            ("STEPCLIMB_STORAGE_S3_ALLOW_HTTP", "true"),
        ]);
        assert_eq!(config.endpoint.as_deref(), Some("http://minio:9000"));
        assert!(config.allow_http);
        let https = s3_config(&[("STEPCLIMB_STORAGE_S3_ENDPOINT", "https://s3.example.org")]);
        assert_eq!(https.endpoint.as_deref(), Some("https://s3.example.org"));
    }

    #[test]
    fn an_endpoint_is_an_origin_only_and_is_never_echoed() {
        for bad in [
            "https://zq-user:zq-pass@s3.example.org",
            "https://s3.example.org/bucket",
            "https://s3.example.org?x=zq-pass",
            "ftp://s3.example.org",
            "zq-pass",
        ] {
            let error = s3(&[("STEPCLIMB_STORAGE_S3_ENDPOINT", bad)]).unwrap_err();
            assert_eq!(refused_at(&error), "STEPCLIMB_STORAGE_S3_ENDPOINT", "{bad}");
            assert!(!error.to_string().contains("zq-pass"), "{error}");
        }
    }

    #[test]
    fn the_region_defaults_and_is_checked() {
        assert_eq!(
            s3_config(&[("STEPCLIMB_STORAGE_S3_REGION", "eu-central-1")]).region,
            "eu-central-1"
        );
        let error = s3(&[("STEPCLIMB_STORAGE_S3_REGION", "EU West")]).unwrap_err();
        assert_eq!(refused_at(&error), "STEPCLIMB_STORAGE_S3_REGION");
    }

    #[test]
    fn the_report_and_debug_hide_the_secret_access_key() {
        let storage = s3(&[
            (STORAGE_URL.var, "s3://stepclimb/lms"),
            ("STEPCLIMB_STORAGE_S3_ENDPOINT", "http://minio:9000"),
            ("STEPCLIMB_STORAGE_S3_ALLOW_HTTP", "true"),
        ])
        .unwrap();
        assert_eq!(
            storage.report(),
            [
                ("storage_url", "s3://stepclimb/lms".to_owned()),
                ("storage_s3_endpoint", "http://minio:9000".to_owned()),
                ("storage_s3_region", "us-east-1".to_owned()),
                ("storage_s3_access_key_id", "test-key".to_owned()),
                ("storage_s3_secret_access_key", "(set, hidden)".to_owned()),
                ("storage_s3_allow_http", "true".to_owned()),
            ]
        );
        assert_eq!(storage.to_string(), "s3://stepclimb/lms");
        let printed = format!("{storage:?} {:?}", storage.report());
        assert!(!printed.contains(SECRET), "{printed}");
        assert!(printed.contains("(hidden)"), "{printed}");
    }
}
