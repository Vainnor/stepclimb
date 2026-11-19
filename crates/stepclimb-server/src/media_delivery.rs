// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /media/{token}/{name}` and `GET /media/{token}/{rendition}/{name}`
//! (docs/specs/m1/08-uploaded-video.md B18, B19): one output of the video run a playback token
//! names ([`crate::media_urls`]), streamed from the store.
//!
//! The routes take no session and no permission: the token is the credential. They refuse, with
//! one answer (404 `file.not_found`) whatever the reason, a token that does not open, has expired
//! or was issued in another organization than the request's; a video that is gone, not `ready`
//! or since transcoded again under another run; and a path that is not an output its run wrote
//! ([`stepclimb_media::served::output`]: no `..`, no other prefix, no rendition the video lacks).
//! The object read is always `orgs/<org>/videos/<video>/v<run>/<path>`, built from the token and
//! that checked path. Each answer carries file delivery's headers (`nosniff`, a sandboxing
//! `Content-Security-Policy`, `Cross-Origin-Resource-Policy: same-origin`,
//! `Referrer-Policy: no-referrer`, `Cache-Control: private` until the token expires, one
//! `Range`), the output's type, and `attachment` for the MP4. `HEAD` gets the same headers.
//! Cross-origin reads (B18a) are [`crate::signed_cors`]'s. The request's path holds the token,
//! so [`crate::app`] writes it to no log.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use object_store::{GetOptions, GetRange, ObjectStoreExt as _, path::Path};
use stepclimb_core::{OrgScope, video_playback};
use stepclimb_media::served::{self, Output};

use crate::{
    AppState,
    error::ApiError,
    extract::ApiPath,
    file_delivery::{Requested, SANDBOX_POLICY, content_disposition, parse_range},
    media_urls::{self, MediaGrant},
    openapi::problem_responses,
    request_org::RequestOrg,
    require::code::FILE_NOT_FOUND,
    storage::Storage,
};

problem_responses! {
    MediaNotFound => "`file.not_found`: the URL is not a playback URL of this server's organization, has expired, names a video that is gone, not ready or transcoded again since, or names a path the video's outputs do not have. Every one of these answers the same.",
    MediaRangeNotSatisfiable => "`file.range_invalid`: the `Range` starts at or after the output's end. `Content-Range` names its length.",
    MediaDeliveryStorageNotConfigured => "`storage.not_configured`: the server has no file storage; `detail` names `STEPCLIMB_STORAGE_URL`. `org.not_found`: the request's organization cannot be determined.",
}

/// An output at the top of the run's prefix: the master playlist, the MP4, the poster, the
/// thumbnail index or a sprite.
#[utoipa::path(
    get,
    path = "/media/{token}/{name}",
    operation_id = "getMediaOutput",
    tag = "videos",
    params(
        ("token" = String, Path, description = "The token of a playback session (`createItemVideoSession`, `createVideoPreviewSession`)."),
        ("name" = String, Path, description = "`master.m3u8`, `video.mp4`, `poster.jpg`, `thumbnails.vtt` or a sprite `sprite_<nnn>.jpg`."),
        ("Range" = Option<String>, Header, description = "One range of bytes, as for `getSignedFile`."),
    ),
    responses(
        (status = 200, description = "The output, as `Content-Type` its type (`application/vnd.apple.mpegurl`, `video/mp4`, `image/jpeg` or `text/vtt`). The MP4 is `Content-Disposition: attachment`. Every answer carries `X-Content-Type-Options: nosniff`, a sandboxing `Content-Security-Policy`, `Cache-Control: private` until the session expires, `Accept-Ranges: bytes`, `Cross-Origin-Resource-Policy: same-origin`, `Referrer-Policy: no-referrer` and, for an `Origin` the server allows, `Access-Control-Allow-Origin`.", body = Vec<u8>, content_type = "application/octet-stream"),
        (status = 206, description = "The bytes the `Range` asks for, with `Content-Range`, and the same headers.", body = Vec<u8>, content_type = "application/octet-stream"),
        (status = 404, response = MediaNotFound),
        (status = 416, response = MediaRangeNotSatisfiable),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = MediaDeliveryStorageNotConfigured),
    )
)]
pub async fn get_media_output(
    storage: Storage,
    org: RequestOrg,
    State(state): State<AppState>,
    ApiPath((token, name)): ApiPath<(String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    serve(
        &storage,
        org.scope(),
        &state,
        (&token, &name),
        (&method, &headers),
    )
    .await
}

/// An output of one rendition: its media playlist or an MPEG-TS segment.
#[utoipa::path(
    get,
    path = "/media/{token}/{rendition}/{name}",
    operation_id = "getMediaRenditionOutput",
    tag = "videos",
    params(
        ("token" = String, Path, description = "The token of a playback session."),
        ("rendition" = String, Path, description = "A rendition the video has: `360p`, `720p`, `1080p`, or one below 360p."),
        ("name" = String, Path, description = "`index.m3u8` or a segment `segment_<nnnnn>.ts`."),
        ("Range" = Option<String>, Header, description = "One range of bytes, as for `getSignedFile`."),
    ),
    responses(
        (status = 200, description = "The playlist (`application/vnd.apple.mpegurl`) or segment (`video/mp2t`), with the headers of `getMediaOutput`.", body = Vec<u8>, content_type = "application/octet-stream"),
        (status = 206, description = "The bytes the `Range` asks for, with `Content-Range`, and the same headers.", body = Vec<u8>, content_type = "application/octet-stream"),
        (status = 404, response = MediaNotFound),
        (status = 416, response = MediaRangeNotSatisfiable),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = MediaDeliveryStorageNotConfigured),
    )
)]
pub async fn get_media_rendition_output(
    storage: Storage,
    org: RequestOrg,
    State(state): State<AppState>,
    ApiPath((token, rendition, name)): ApiPath<(String, String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let path = format!("{rendition}/{name}");
    serve(
        &storage,
        org.scope(),
        &state,
        (&token, &path),
        (&method, &headers),
    )
    .await
}

/// The output `path` of the run `token` names, in the request's organization `scope`.
async fn serve(
    storage: &Storage,
    scope: OrgScope,
    state: &AppState,
    (token, path): (&str, &str),
    (method, headers): (&Method, &HeaderMap),
) -> Result<Response, ApiError> {
    let now = state.clock.now();
    let grant = media_urls::open(&state.keys, token)
        .filter(|grant| grant.org_id == scope.org_id() && grant.valid_at(now))
        .ok_or_else(not_found)?;
    let output = video_playback::output(&state.db, scope, grant.video_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .filter(|output| output.serves(grant.run))
        .ok_or_else(not_found)?;
    let kind = served::output(path, &output.heights).ok_or_else(not_found)?;
    let location = object(storage, scope, &grant, path);
    let meta = match storage.store().head(&location).await {
        Ok(meta) => meta,
        Err(object_store::Error::NotFound { .. }) => return Err(not_found()),
        Err(error) => return Err(ApiError::internal(&error)),
    };
    let size = meta.size;
    let mut answer = output_headers(kind, grant.seconds_left(now))?;
    let range = match headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        Some(value) => parse_range(value, size),
        None => Requested::Whole,
    };
    let (status, start, end) = match range {
        Requested::Whole => (StatusCode::OK, 0, size),
        Requested::Part { start, end } => {
            let content_range = format!("bytes {start}-{}/{size}", end - 1);
            answer.insert(header::CONTENT_RANGE, header_value(&content_range)?);
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        Requested::Unsatisfiable => return Ok(unsatisfiable(size)),
    };
    answer.insert(header::CONTENT_LENGTH, HeaderValue::from(end - start));
    let body = if start == end || method == Method::HEAD {
        Body::empty()
    } else {
        let options = GetOptions {
            range: Some(GetRange::Bounded(start..end)),
            ..GetOptions::default()
        };
        let got = storage
            .store()
            .get_opts(&location, options)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        Body::from_stream(got.into_stream())
    };
    Ok((status, answer, body).into_response())
}

/// `orgs/<org>/videos/<video>/v<run>/<path>`: the scope's organization, the token's video and
/// run, and a `path` [`served::output`] accepted.
fn object(storage: &Storage, scope: OrgScope, grant: &MediaGrant, path: &str) -> Path {
    let prefix = storage.video_prefix(scope, grant.video_id);
    let run = format!("v{}", grant.run);
    Path::from_iter(
        prefix
            .parts()
            .chain(std::iter::once(run.as_str().into()))
            .chain(path.split('/').map(Into::into)),
    )
}

/// 404 `file.not_found`, the one answer for every request that does not serve an output.
fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        FILE_NOT_FOUND,
        "The link is not valid, has expired, or names a video or file that is not available.",
    )
}

/// 416 `file.range_invalid` with `Content-Range: bytes */<size>`.
fn unsatisfiable(size: u64) -> Response {
    let mut response = ApiError::new(
        StatusCode::RANGE_NOT_SATISFIABLE,
        crate::file_delivery::code::RANGE_INVALID,
        "The range asks for bytes past the output's end.",
    )
    .into_response();
    if let Ok(value) = HeaderValue::from_str(&format!("bytes */{size}")) {
        response.headers_mut().insert(header::CONTENT_RANGE, value);
    }
    response
}

fn header_value(value: &str) -> Result<HeaderValue, ApiError> {
    HeaderValue::from_str(value).map_err(|error| ApiError::internal(&error))
}

/// The headers of every answer that serves an output of `kind` (B18).
fn output_headers(kind: Output, seconds_left: u64) -> Result<HeaderMap, ApiError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(kind.content_type()),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if kind == Output::Mp4 {
        headers.insert(
            header::CONTENT_DISPOSITION,
            header_value(&content_disposition(false, stepclimb_media::plan::MP4))?,
        );
    }
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(SANDBOX_POLICY),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header_value(&format!("private, max-age={seconds_left}"))?,
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    Ok(headers)
}
