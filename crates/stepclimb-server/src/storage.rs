// SPDX-License-Identifier: AGPL-3.0-or-later
//! Object storage for course files (docs/specs/m1/06-files.md B1, B1a, B2).
//!
//! [`Storage::open`] builds one `object_store` store from [`StorageConfig`]: a local directory
//! (`file://`, written with `fsync`, so an object a committed `files` row names survives a
//! crash) or S3 and S3-compatible stores (`s3://`). `stepclimb serve` opens it and runs
//! [`Storage::probe`] before it listens, so a store it cannot write, read and delete stops
//! startup with the failing step named. Without `storage_url` there is no [`Storage`]: the
//! server starts, and a handler that takes [`Storage`] as an extractor answers 503
//! `storage.not_configured` naming `STEPCLIMB_STORAGE_URL` (B1a), which is an error on each use,
//! never a fallback to some other place.
//!
//! Every object key is under the store's prefix, and a file's key is
//! `stepclimb_core::files::storage_key`: the organization and the SHA-256, nothing a client
//! sent (B2).
//!
//! The S3 client's HTTP transport is the server's own ([`Transport`]): reqwest with the rustls,
//! ring and platform-verifier setup of `stepclimb_auth::outbound`, no proxy, no redirects and
//! HTTP/1.1, so the store adds no second TLS stack. The store is configured by the operator, not
//! entered by an admin, so the outbound URL policy does not apply: MinIO on a private network is
//! the expected case.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};
use http_body_util::BodyExt as _;
use object_store::{
    ObjectStore, ObjectStoreExt as _, PutPayload, RetryConfig,
    aws::AmazonS3Builder,
    client::{
        ClientOptions, HttpClient, HttpConnector, HttpError, HttpErrorKind, HttpRequest,
        HttpResponse, HttpResponseBody, HttpService,
    },
    local::LocalFileSystem,
    path::{Path, PathPart},
};
use stepclimb_core::{
    OrgScope,
    files::{self, Sha256Digest},
    id::{Uuid, new_id},
};

use crate::{
    AppState,
    config::{S3Config, STORAGE_URL, StorageConfig},
    error::ApiError,
};

/// The codes this module produces.
pub mod codes {
    /// 503: `storage_url` is not set, so stored bytes cannot be read or written (B1a).
    pub const NOT_CONFIGURED: &str = "storage.not_configured";
}

/// The `detail` of [`codes::NOT_CONFIGURED`], naming the setting to fix.
pub const NOT_CONFIGURED_DETAIL: &str =
    "object storage is not configured on this server; the operator sets STEPCLIMB_STORAGE_URL";

/// The prefix, under the store's own prefix, of the objects [`Storage::probe`] writes.
pub const PROBE_PREFIX: &str = ".probe";

/// How long the S3 client waits to connect, and for each read of a response.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How the S3 client retries a request that failed in transit or with a 5xx: object_store's
/// backoff, at most 3 retries within 15 seconds, so a store that is down stops startup quickly.
fn retry_config() -> RetryConfig {
    RetryConfig {
        max_retries: 3,
        retry_timeout: Duration::from_secs(15),
        ..RetryConfig::default()
    }
}

/// How the upload route bounds the uploads it serves (docs/specs/m1/06-files.md B5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadLimits {
    /// How long an upload's body may go without sending a frame before the upload is refused
    /// and its multipart upload aborted.
    pub idle: Duration,
    /// How many uploads one server streams at once; one more answers 503 `file.uploads_busy`.
    pub in_flight: usize,
}

impl UploadLimits {
    /// 30 seconds without a frame, 16 uploads at once. With 5 MiB parts an upload holds about
    /// 15 MiB, so 16 hold about 240 MiB.
    pub const DEFAULT: Self = Self {
        idle: Duration::from_secs(30),
        in_flight: 16,
    };
}

/// The configured store, the prefix every key lives under, the largest file one upload may send
/// (`max_file_bytes`, B4), and the uploads in flight, which only the upload route reads. Cloning
/// is cheap, and clones share the uploads in flight.
#[derive(Clone)]
pub struct Storage {
    store: Arc<dyn ObjectStore>,
    prefix: Path,
    max_file_bytes: u64,
    limits: UploadLimits,
    uploads: Arc<Semaphore>,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("store", &self.store.to_string())
            .field("prefix", &self.prefix.as_ref())
            .field("max_file_bytes", &self.max_file_bytes)
            .field("limits", &self.limits)
            .finish()
    }
}

/// Why storage could not be opened or did not pass the probe. No variant carries a credential.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The store `storage_url` names could not be set up, such as a local directory that does
    /// not exist.
    #[error("could not open the store {var} names: {source}", var = STORAGE_URL.var)]
    Open {
        #[source]
        source: object_store::Error,
    },
    /// The probe could not write, read or delete its object.
    #[error("the storage probe could not {step} {key}: {source}")]
    Probe {
        step: &'static str,
        key: String,
        #[source]
        source: object_store::Error,
    },
    /// The probe read back other bytes than it wrote.
    #[error("the storage probe read back other bytes from {key} than it wrote")]
    ProbeMismatch { key: String },
}

impl Storage {
    /// The store `config` names, with uploads of at most `max_file_bytes` (`Config`'s value), or
    /// `None` when storage is not configured. Builds the client only; nothing is sent until
    /// [`Storage::probe`] or a later call.
    pub fn open(config: &StorageConfig, max_file_bytes: u64) -> Result<Option<Self>, StorageError> {
        let open = |source| StorageError::Open { source };
        let (store, prefix): (Arc<dyn ObjectStore>, &str) = match config {
            StorageConfig::NotConfigured => return Ok(None),
            StorageConfig::Local { root } => {
                let local = LocalFileSystem::new_with_prefix(root)
                    .map_err(open)?
                    .with_fsync(true);
                (Arc::new(local), "")
            }
            StorageConfig::S3(s3) => (Arc::new(s3_store(s3).map_err(open)?), &s3.prefix),
        };
        Ok(Some(Self {
            store,
            prefix: Path::from_iter(prefix.split('/').filter(|s| !s.is_empty())),
            max_file_bytes,
            limits: UploadLimits::DEFAULT,
            uploads: Arc::new(Semaphore::new(UploadLimits::DEFAULT.in_flight)),
        }))
    }

    /// This store with `limits` instead of [`UploadLimits::DEFAULT`], and uploads in flight of
    /// its own.
    #[must_use]
    pub fn with_upload_limits(self, limits: UploadLimits) -> Self {
        Self {
            limits,
            uploads: Arc::new(Semaphore::new(limits.in_flight)),
            ..self
        }
    }

    /// The bounds of the uploads this store serves.
    #[must_use]
    pub fn upload_limits(&self) -> UploadLimits {
        self.limits
    }

    /// A place for one more upload in flight, held until the permit drops, or `None` when
    /// [`UploadLimits::in_flight`] uploads are already streaming.
    #[must_use]
    pub fn try_start_upload(&self) -> Option<OwnedSemaphorePermit> {
        self.uploads.clone().try_acquire_owned().ok()
    }

    /// The largest file one upload may send, in bytes (`max_file_bytes`, B4).
    #[must_use]
    pub fn max_file_bytes(&self) -> u64 {
        self.max_file_bytes
    }

    /// The store, for reading and writing objects at [`Storage::path`]s.
    #[must_use]
    pub fn store(&self) -> &Arc<dyn ObjectStore> {
        &self.store
    }

    /// The location of `key`, a `/`-separated key the server built, under the store's prefix.
    #[must_use]
    pub fn path(&self, key: &str) -> Path {
        let parts = key.split('/').filter(|s| !s.is_empty()).map(PathPart::from);
        Path::from_iter(self.prefix.parts().chain(parts))
    }

    /// The location of the scope's organization's file with `sha256` (B2).
    #[must_use]
    pub fn file_path(&self, scope: OrgScope, sha256: Sha256Digest) -> Path {
        self.path(&files::storage_key(scope, sha256))
    }

    /// The location of the temporary object an upload streams to,
    /// `orgs/<org_id>/uploads/<upload_id>` (B5).
    #[must_use]
    pub fn upload_path(&self, scope: OrgScope, upload_id: Uuid) -> Path {
        self.path(&format!("{}/{upload_id}", Self::uploads_key(scope)))
    }

    /// Where the scope's organization's temporary upload objects are (B8).
    #[must_use]
    pub fn uploads_prefix(&self, scope: OrgScope) -> Path {
        self.path(&Self::uploads_key(scope))
    }

    /// Where the scope's organization's file objects are (B2, B21).
    #[must_use]
    pub fn files_prefix(&self, scope: OrgScope) -> Path {
        self.path(&format!("orgs/{}/files", scope.org_id()))
    }

    fn uploads_key(scope: OrgScope) -> String {
        format!("orgs/{}/uploads", scope.org_id())
    }

    /// The key of `location` relative to the store's prefix, as `files::storage_key` writes
    /// one; `None` for a location outside the prefix.
    #[must_use]
    pub fn key_of(&self, location: &Path) -> Option<String> {
        let parts: Vec<_> = location
            .prefix_match(&self.prefix)?
            .map(|p| p.as_ref().to_owned())
            .collect();
        Some(parts.join("/"))
    }

    /// Write, read back and delete an object under [`PROBE_PREFIX`] (B1). The first step that
    /// fails is the error. When reading fails or returns other bytes, the object is still
    /// deleted, and a failed delete is logged with the probe's error returned.
    pub async fn probe(&self) -> Result<(), StorageError> {
        let id = new_id();
        let key = self.path(&format!("{PROBE_PREFIX}/{id}"));
        let written = format!("stepclimb storage probe {id}");
        let failed = |step, source| StorageError::Probe {
            step,
            key: key.to_string(),
            source,
        };
        self.store
            .put(&key, PutPayload::from(written.clone().into_bytes()))
            .await
            .map_err(|source| failed("write", source))?;
        let read = match self.store.get(&key).await {
            Ok(result) => result.bytes().await,
            Err(error) => Err(error),
        };
        let checked = match read {
            Ok(bytes) if bytes == written.as_bytes() => Ok(()),
            Ok(_) => Err(StorageError::ProbeMismatch {
                key: key.to_string(),
            }),
            Err(source) => Err(failed("read", source)),
        };
        let deleted = self.store.delete(&key).await;
        match (checked, deleted) {
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(source)) => Err(failed("delete", source)),
            (Err(error), Ok(())) => Err(error),
            (Err(error), Err(delete)) => {
                tracing::warn!(key = %key, error = %delete, "could not delete the storage probe object");
                Err(error)
            }
        }
    }
}

/// The S3 store for `s3`: static credentials only, path-style requests (MinIO's default), and
/// the server's own transport.
fn s3_store(s3: &S3Config) -> object_store::Result<object_store::aws::AmazonS3> {
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&s3.bucket)
        .with_region(&s3.region)
        .with_access_key_id(&s3.access_key_id)
        .with_secret_access_key(&s3.secret_access_key)
        .with_allow_http(s3.allow_http)
        .with_virtual_hosted_style_request(false)
        .with_retry(retry_config())
        .with_http_connector(Transport {
            allow_http: s3.allow_http,
        });
    if let Some(endpoint) = &s3.endpoint {
        builder = builder.with_endpoint(endpoint);
    }
    builder.build()
}

/// The S3 client's HTTP connector: a reqwest client with `stepclimb_auth::outbound`'s TLS
/// setup, `https` only unless `allow_http`, no proxy and no redirects. object_store's own
/// [`ClientOptions`] are not read: these settings are fixed here.
#[derive(Debug)]
struct Transport {
    allow_http: bool,
}

impl HttpConnector for Transport {
    fn connect(&self, _options: &ClientOptions) -> object_store::Result<HttpClient> {
        let generic =
            |source: Box<dyn std::error::Error + Send + Sync>| object_store::Error::Generic {
                store: "S3",
                source,
            };
        let tls = stepclimb_auth::outbound::tls_config().map_err(|e| generic(Box::new(e)))?;
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(!self.allow_http)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .build()
            .map_err(|e| generic(Box::new(e.without_url())))?;
        Ok(HttpClient::new(ReqwestService(client)))
    }
}

/// object_store's request through a reqwest client, and reqwest's response back.
#[derive(Debug)]
struct ReqwestService(reqwest::Client);

#[async_trait::async_trait]
impl HttpService for ReqwestService {
    async fn call(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let (parts, body) = request.into_parts();
        let request = http::Request::from_parts(parts, reqwest::Body::wrap(body));
        let request = reqwest::Request::try_from(request).map_err(transport_error)?;
        let response = self.0.execute(request).await.map_err(transport_error)?;
        let (parts, body) = http::Response::<reqwest::Body>::from(response).into_parts();
        let body = HttpResponseBody::new(body.map_err(transport_error));
        Ok(HttpResponse::from_parts(parts, body))
    }
}

/// A reqwest error as object_store's, without the URL; the kind decides whether object_store
/// retries.
fn transport_error(error: reqwest::Error) -> HttpError {
    let kind = if error.is_timeout() {
        HttpErrorKind::Timeout
    } else if error.is_connect() {
        HttpErrorKind::Connect
    } else if error.is_request() || error.is_body() {
        HttpErrorKind::Request
    } else if error.is_decode() {
        HttpErrorKind::Decode
    } else {
        HttpErrorKind::Unknown
    };
    HttpError::new(kind, error.without_url())
}

/// The 503 of a route that needs stored bytes when storage is not configured (B1a).
#[must_use]
pub fn not_configured() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        codes::NOT_CONFIGURED,
        NOT_CONFIGURED_DETAIL,
    )
}

/// A handler that reads or writes stored bytes takes [`Storage`]; without `storage_url` the
/// request is refused with [`not_configured`] before the handler runs. A protected handler takes
/// it after its `Require<P>`: axum runs extractors in argument order, so an anonymous caller gets
/// 401 and a caller without the permission 403 before storage is looked at.
impl FromRequestParts<AppState> for Storage {
    type Rejection = ApiError;

    async fn from_request_parts(
        _parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        state.storage.clone().ok_or_else(not_configured)
    }
}
