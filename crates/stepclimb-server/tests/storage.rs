// SPDX-License-Identifier: AGPL-3.0-or-later
//! Object storage (docs/specs/m1/06-files.md B1, B1a, B2; T1, T1a): the startup probe, where a
//! file's object lives, and the 503 a route that needs stored bytes answers without storage.
//!
//! The local-directory cases use a temporary directory. The S3 cases run against the MinIO in
//! the gate's own Compose project, whose URL `bin/ci_gate.py` passes in `SC_CI_MINIO_URL` with
//! the bucket `stepclimb` already made (deploy/compose/ci.yaml); each test works under a prefix
//! of its own, so tests running at once never meet. No case reaches the network outside it.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use http_body_util::BodyExt as _;
use object_store::{ObjectStoreExt as _, PutPayload};
use sqlx::postgres::PgPoolOptions;
use stepclimb_auth::encryption::KeyRing;
use stepclimb_core::{
    OrgScope,
    clock::ManualClock,
    files::Sha256,
    id::{Uuid, new_id},
};
use stepclimb_server::{
    AppState,
    config::{S3Config, StorageConfig},
    storage::{Storage, StorageError},
    web_origin::WebOrigin,
};
use tower::ServiceExt as _;

/// Every object key under `prefix`, walking object_store's delimiter listing, sorted.
async fn list_keys(storage: &Storage, prefix: &object_store::path::Path) -> Vec<String> {
    let mut keys = Vec::new();
    let mut pending = vec![prefix.clone()];
    while let Some(at) = pending.pop() {
        let page = storage
            .store()
            .list_with_delimiter(Some(&at))
            .await
            .expect("list");
        keys.extend(page.objects.into_iter().map(|o| o.location.to_string()));
        pending.extend(page.common_prefixes);
    }
    keys.sort();
    keys
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the credential resolvers"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// A directory of its own under the system's temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("stepclimb-storage-{test}-{}", new_id()));
        std::fs::create_dir_all(&dir).expect("create the directory");
        Self(dir)
    }

    fn storage(&self) -> Storage {
        Storage::open(&StorageConfig::Local {
            root: self.0.clone(),
        })
        .expect("open the directory")
        .expect("storage is configured")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The run's MinIO, under a prefix of the test's own.
fn minio(prefix: &str) -> S3Config {
    let endpoint = std::env::var("SC_CI_MINIO_URL").expect(
        "SC_CI_MINIO_URL names the gate's MinIO: run the storage tests inside `just ci` or \
         `python3 bin/ci_gate.py run -- cargo nextest run -p stepclimb-server --test storage`",
    );
    S3Config {
        bucket: "stepclimb".to_owned(),
        prefix: prefix.to_owned(),
        endpoint: Some(endpoint),
        region: "us-east-1".to_owned(),
        // deploy/compose/compose.yaml's development credentials.
        access_key_id: "stepclimb".to_owned(),
        secret_access_key: "stepclimb-local".to_owned(),
        allow_http: true,
    }
}

fn open(config: S3Config) -> Storage {
    Storage::open(&StorageConfig::S3(config))
        .expect("build the S3 client")
        .expect("storage is configured")
}

fn test_prefix() -> String {
    format!("tests/{}", new_id().simple())
}

#[test]
fn without_a_storage_url_there_is_no_store() {
    assert!(
        Storage::open(&StorageConfig::NotConfigured)
            .expect("nothing to open")
            .is_none()
    );
}

#[tokio::test]
async fn a_local_directory_passes_the_probe_and_keeps_no_probe_object() {
    let dir = TempDir::new("probe");
    dir.storage().probe().await.expect("the probe passes");
    let probes = dir.0.join(".probe");
    let left = std::fs::read_dir(&probes).map_or(0, |entries| entries.count());
    assert_eq!(left, 0, "objects left under {}", probes.display());
}

#[tokio::test]
async fn a_local_directory_that_does_not_exist_is_refused_naming_the_setting() {
    let missing = std::env::temp_dir().join(format!("stepclimb-storage-missing-{}", new_id()));
    let error = Storage::open(&StorageConfig::Local { root: missing }).unwrap_err();
    assert!(matches!(error, StorageError::Open { .. }), "{error}");
    assert!(
        error.to_string().contains("STEPCLIMB_STORAGE_URL"),
        "{error}"
    );
}

#[tokio::test]
async fn a_directory_the_server_cannot_write_fails_the_probe_at_the_write() {
    let dir = TempDir::new("read-only");
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    let error = dir.storage().probe().await.unwrap_err();
    assert!(
        matches!(error, StorageError::Probe { step: "write", .. }),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("the storage probe could not write .probe/"),
        "{error}"
    );
}

#[tokio::test]
async fn a_files_object_is_at_its_organizations_hash_key() {
    let dir = TempDir::new("layout");
    let storage = dir.storage();
    let org = new_id();
    let sha256 = Sha256([0x5a; 32]);
    let path = storage.file_path(scope(org), sha256);
    storage
        .store()
        .put(&path, PutPayload::from_static(b"bytes"))
        .await
        .expect("put");
    let on_disk = dir
        .0
        .join("orgs")
        .join(org.to_string())
        .join("files")
        .join("5a".repeat(32));
    assert_eq!(std::fs::read(on_disk).expect("the object's file"), b"bytes");
}

#[tokio::test]
async fn minio_passes_the_probe_and_keeps_no_probe_object() {
    let prefix = test_prefix();
    let storage = open(minio(&prefix));
    storage.probe().await.expect("the probe passes");
    let whole_bucket = open(minio(""));
    let left = list_keys(&whole_bucket, &whole_bucket.path(&prefix)).await;
    assert_eq!(left, Vec::<String>::new());
}

#[tokio::test]
async fn an_s3_files_object_is_under_the_prefix_at_its_organizations_hash_key() {
    let prefix = test_prefix();
    let storage = open(minio(&prefix));
    let org = new_id();
    let sha256 = Sha256([0x0f; 32]);
    storage
        .store()
        .put(
            &storage.file_path(scope(org), sha256),
            PutPayload::from_static(b"pdf"),
        )
        .await
        .expect("put");
    let whole_bucket = open(minio(""));
    assert_eq!(
        list_keys(&whole_bucket, &whole_bucket.path(&prefix)).await,
        [format!("{prefix}/orgs/{org}/files/{}", "0f".repeat(32))]
    );
    let read = whole_bucket
        .store()
        .get(&whole_bucket.path(&format!("{prefix}/orgs/{org}/files/{}", "0f".repeat(32))))
        .await
        .expect("get")
        .bytes()
        .await
        .expect("bytes");
    assert_eq!(&read[..], b"pdf");
}

#[tokio::test]
async fn a_bucket_that_does_not_exist_fails_the_probe() {
    let config = S3Config {
        bucket: format!("missing-{}", new_id().simple()),
        ..minio(&test_prefix())
    };
    let error = open(config).probe().await.unwrap_err();
    assert!(
        matches!(error, StorageError::Probe { step: "write", .. }),
        "{error}"
    );
}

#[tokio::test]
async fn a_wrong_secret_fails_the_probe_without_printing_it() {
    let config = S3Config {
        secret_access_key: "zq-wrong-secret".to_owned(),
        ..minio(&test_prefix())
    };
    let error = open(config).probe().await.unwrap_err();
    assert!(
        matches!(error, StorageError::Probe { step: "write", .. }),
        "{error}"
    );
    let printed = format!("{error} {error:?}");
    assert!(!printed.contains("zq-wrong-secret"), "{printed}");
}

#[tokio::test]
async fn plain_http_is_refused_unless_allowed() {
    let config = S3Config {
        allow_http: false,
        ..minio(&test_prefix())
    };
    let error = open(config).probe().await.unwrap_err();
    assert!(
        matches!(error, StorageError::Probe { step: "write", .. }),
        "{error}"
    );
}

/// Application state with `storage` and a pool that points nowhere: the route below never
/// touches the database.
fn state(storage: Option<Storage>) -> AppState {
    let db = PgPoolOptions::new()
        .connect_lazy("postgres://stepclimb@127.0.0.1:1/none")
        .expect("a lazy pool parses its URL");
    AppState {
        db,
        clock: ManualClock::new(std::time::SystemTime::UNIX_EPOCH).clock(),
        keys: KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            .expect("a valid test key"),
        public_url: WebOrigin::parse("http://localhost:8080").expect("a valid origin"),
        allowed_origins: Vec::new(),
        rate_limits: Default::default(),
        passwords: Default::default(),
        storage,
        providers: stepclimb_server::providers::Providers::new(false).expect("provider clients"),
    }
}

/// A route that needs stored bytes, as the upload and download routes will.
async fn needs_storage(storage: Option<Storage>) -> (StatusCode, String) {
    let app = Router::new()
        .route("/stored", get(|_: Storage| async { "reached" }))
        .with_state(state(storage));
    let response = app
        .oneshot(
            Request::get("/stored")
                .body(Body::empty())
                .expect("a request"),
        )
        .await
        .expect("the router never fails");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("the body")
        .to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn a_route_that_needs_storage_answers_503_naming_the_setting_without_it() {
    let (status, body) = needs_storage(None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    let problem: serde_json::Value = serde_json::from_str(&body).expect("problem details");
    assert_eq!(problem["code"], "storage.not_configured");
    assert_eq!(problem["status"], 503);
    let detail = problem["detail"].as_str().expect("a detail");
    assert!(detail.contains("STEPCLIMB_STORAGE_URL"), "{detail}");
}

#[tokio::test]
async fn a_route_that_needs_storage_runs_with_it() {
    let dir = TempDir::new("route");
    let (status, body) = needs_storage(Some(dir.storage())).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "reached"));
}
