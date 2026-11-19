// SPDX-License-Identifier: AGPL-3.0-or-later
//! `GET /files/{token}` (docs/specs/m1/06-files.md B17, Q8): the bytes of the course file a
//! signed URL names ([`crate::file_urls`]), streamed from the store with headers that stop the
//! browser from running them.
//!
//! The route takes no session and no permission: the token is the credential. It refuses, with
//! one answer (404 `file.not_found`) whatever the reason, a token that does not open, has
//! expired, was issued in another organization than the request's, or names a course file that
//! no longer exists, so a token tells its holder nothing. It honors one `Range` and
//! `If-None-Match`, and answers `HEAD` with the same headers without reading the store. The
//! request's path holds the token, so [`crate::app`] writes it to no log.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use object_store::{GetOptions, GetRange};
use stepclimb_core::course_files;

use crate::{
    AppState,
    error::ApiError,
    extract::ApiPath,
    file_urls::{self, Disposition, Grant},
    openapi::problem_responses,
    request_org::RequestOrg,
    require::code::FILE_NOT_FOUND,
    storage::Storage,
};

/// The codes this module produces besides `file.not_found`.
pub mod code {
    /// The `Range` asks for bytes the file does not have (416).
    pub const RANGE_INVALID: &str = "file.range_invalid";
}

/// The types a browser may show in the page; every other type is an attachment (B17).
const INLINE_TYPES: [&str; 5] = [
    "application/pdf",
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
];

/// The policy of a PDF: nothing loads or runs, and no `sandbox`, under which browsers refuse to
/// show a PDF in a frame (Q8).
pub const PDF_POLICY: &str = "default-src 'none'";

/// The policy of every other type: nothing loads or runs, and the document is sandboxed.
pub const SANDBOX_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; sandbox";

problem_responses! {
    FileNotFound => "`file.not_found`: the URL is not a signed file URL of this server's organization, has expired, or names a file that no longer exists. Every one of these answers the same.",
    RangeNotSatisfiable => "`file.range_invalid`: the `Range` starts at or after the file's end. `Content-Range` names the file's length.",
    FileStorageNotConfigured => "`storage.not_configured`: the server has no file storage; `detail` names `STEPCLIMB_STORAGE_URL`. `org.not_found`: the request's organization cannot be determined.",
}

/// The file a signed URL names.
#[utoipa::path(
    get,
    path = "/files/{token}",
    operation_id = "getSignedFile",
    tag = "files",
    params(
        ("token" = String, Path, description = "The token of a signed URL (`createItemFileUrl`, `createCourseFileUrl`)."),
        ("Range" = Option<String>, Header, description = "One range of bytes, `bytes=<first>-<last>`, `bytes=<first>-` or `bytes=-<length>`. Several ranges are answered with the whole file."),
        ("If-None-Match" = Option<String>, Header, description = "The `ETag` of a copy the client holds."),
    ),
    responses(
        (status = 200, description = "The whole file, as `Content-Type` its stored type. `Content-Disposition` is `inline` only for an inline URL of a PDF or a PNG, JPEG, GIF or WebP image, and `attachment` with the file's name otherwise. Every answer carries `X-Content-Type-Options: nosniff`, a `Content-Security-Policy` (sandboxed for every type but PDF), `Cache-Control: private` until the URL expires, `ETag` (the SHA-256), `Accept-Ranges: bytes`, `Cross-Origin-Resource-Policy: same-origin` and `Referrer-Policy: no-referrer`.", body = Vec<u8>, content_type = "application/octet-stream"),
        (status = 206, description = "The bytes the `Range` asks for, with `Content-Range`, and the same headers.", body = Vec<u8>, content_type = "application/octet-stream"),
        (status = 304, description = "The `If-None-Match` names the file's `ETag`."),
        (status = 404, response = FileNotFound),
        (status = 416, response = RangeNotSatisfiable),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = FileStorageNotConfigured),
    )
)]
pub async fn get_signed_file(
    storage: Storage,
    org: RequestOrg,
    State(state): State<AppState>,
    ApiPath(token): ApiPath<String>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    let now = state.clock.now();
    let grant = file_urls::open(&state.keys, &token)
        .filter(|grant| grant.org_id == scope.org_id() && grant.valid_at(now))
        .ok_or_else(not_found)?;
    let file = course_files::get(&state.db, scope, grant.course_file_id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .filter(|file| file.file_id == grant.file_id)
        .ok_or_else(not_found)?;
    let mut answer = common_headers(&file, &grant, grant.seconds_left(now))?;
    if none_match(&headers, &etag(&file)) {
        return Ok((StatusCode::NOT_MODIFIED, answer).into_response());
    }
    let range = match headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        Some(value) => parse_range(value, file.size),
        None => Requested::Whole,
    };
    let (status, start, end) = match range {
        Requested::Whole => (StatusCode::OK, 0, file.size),
        Requested::Part { start, end } => {
            let content_range = format!("bytes {start}-{}/{}", end - 1, file.size);
            answer.insert(header::CONTENT_RANGE, header_value(&content_range)?);
            (StatusCode::PARTIAL_CONTENT, start, end)
        }
        Requested::Unsatisfiable => return Ok(unsatisfiable(file.size)),
    };
    answer.insert(header::CONTENT_LENGTH, HeaderValue::from(end - start));
    // A `HEAD` gets the same headers without the store being read; axum would drop a body.
    let body = if start == end || method == Method::HEAD {
        Body::empty()
    } else {
        let options = GetOptions {
            range: Some(GetRange::Bounded(start..end)),
            ..GetOptions::default()
        };
        let path = storage.file_path(scope, file.sha256);
        let got = storage
            .store()
            .get_opts(&path, options)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        Body::from_stream(got.into_stream())
    };
    Ok((status, answer, body).into_response())
}

/// 404 `file.not_found`, the one answer for every token that does not serve a file.
fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        FILE_NOT_FOUND,
        "The link is not valid, has expired, or names a file that no longer exists.",
    )
}

/// 416 `file.range_invalid` with `Content-Range: bytes */<size>`.
fn unsatisfiable(size: u64) -> Response {
    let mut response = ApiError::new(
        StatusCode::RANGE_NOT_SATISFIABLE,
        code::RANGE_INVALID,
        "The range asks for bytes past the file's end.",
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

/// The file's entity tag: its SHA-256, quoted.
fn etag(file: &course_files::CourseFile) -> String {
    format!("\"{}\"", file.sha256.hex())
}

/// The headers of every answer that serves or confirms the file (B17).
fn common_headers(
    file: &course_files::CourseFile,
    grant: &Grant,
    seconds_left: u64,
) -> Result<HeaderMap, ApiError> {
    let inline =
        grant.disposition == Disposition::Inline && INLINE_TYPES.contains(&file.mime.as_str());
    let policy = if file.mime == "application/pdf" {
        PDF_POLICY
    } else {
        SANDBOX_POLICY
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, header_value(&file.mime)?);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header_value(&content_disposition(inline, &file.name))?,
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(policy),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header_value(&format!("private, max-age={seconds_left}"))?,
    );
    headers.insert(header::ETAG, header_value(&etag(file))?);
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

/// `inline` or `attachment` with `name` as RFC 6266 asks: `filename` an ASCII stand-in (every
/// other character, `"` and `\` become `_`) and `filename*` the name in UTF-8, percent-encoded.
#[must_use]
pub fn content_disposition(inline: bool, name: &str) -> String {
    let kind = if inline { "inline" } else { "attachment" };
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            let attr_char = b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'!' | b'#'
                        | b'$'
                        | b'&'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                );
            if attr_char {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

/// Whether `If-None-Match` names `etag` (weakly, as RFC 9110 compares it there) or is `*`.
fn none_match(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.trim_start_matches("W/") == etag)
}

/// What a `Range` header asks of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requested {
    /// No usable range: the whole file. A header this server does not take (another unit,
    /// several ranges, a malformed one) is ignored, as RFC 9110 allows.
    Whole,
    /// The bytes `start..end`.
    Part { start: u64, end: u64 },
    /// One range that starts at or after the file's end, or asks for its last 0 bytes.
    Unsatisfiable,
}

/// The one range `value` asks of a file of `size` bytes (RFC 9110 section 14.1.2).
#[must_use]
pub fn parse_range(value: &str, size: u64) -> Requested {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return Requested::Whole;
    };
    if spec.contains(',') {
        return Requested::Whole;
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return Requested::Whole;
    };
    let number = |text: &str| -> Option<u64> {
        let text = text.trim();
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
            .then(|| text.parse().ok())
            .flatten()
    };
    match (number(first), last.trim().is_empty(), number(last)) {
        // `bytes=-<length>`: the last bytes.
        (None, false, Some(length)) if first.trim().is_empty() => {
            if length == 0 || size == 0 {
                Requested::Unsatisfiable
            } else {
                Requested::Part {
                    start: size - length.min(size),
                    end: size,
                }
            }
        }
        // `bytes=<first>-`.
        (Some(start), true, None) => {
            if start >= size {
                Requested::Unsatisfiable
            } else {
                Requested::Part { start, end: size }
            }
        }
        // `bytes=<first>-<last>`.
        (Some(start), false, Some(last)) if start <= last => {
            if start >= size {
                Requested::Unsatisfiable
            } else {
                Requested::Part {
                    start,
                    end: last.saturating_add(1).min(size),
                }
            }
        }
        _ => Requested::Whole,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_range_is_read_as_rfc_9110_says() {
        use Requested::{Part, Unsatisfiable, Whole};
        let cases = [
            ("bytes=0-99", Part { start: 0, end: 100 }),
            (
                "bytes=10-",
                Part {
                    start: 10,
                    end: 1000,
                },
            ),
            (
                "bytes=-100",
                Part {
                    start: 900,
                    end: 1000,
                },
            ),
            (
                "bytes=-5000",
                Part {
                    start: 0,
                    end: 1000,
                },
            ),
            (
                "bytes=990-5000",
                Part {
                    start: 990,
                    end: 1000,
                },
            ),
            (
                "bytes=999-999",
                Part {
                    start: 999,
                    end: 1000,
                },
            ),
            ("bytes=1000-", Unsatisfiable),
            ("bytes=999999-", Unsatisfiable),
            ("bytes=1000-1001", Unsatisfiable),
            ("bytes=-0", Unsatisfiable),
            ("bytes=5-2", Whole),
            ("bytes=0-1,5-6", Whole),
            ("items=0-1", Whole),
            ("bytes=a-b", Whole),
            ("bytes=-", Whole),
            ("bytes=+1-2", Whole),
        ];
        for (value, expected) in cases {
            assert_eq!(parse_range(value, 1000), expected, "{value}");
        }
        assert_eq!(parse_range("bytes=0-", 0), Unsatisfiable);
        assert_eq!(parse_range("bytes=-1", 0), Unsatisfiable);
    }

    #[test]
    fn a_name_is_sent_in_ascii_and_in_utf_8() {
        assert_eq!(
            content_disposition(true, "Handbook.pdf"),
            "inline; filename=\"Handbook.pdf\"; filename*=UTF-8''Handbook.pdf"
        );
        assert_eq!(
            content_disposition(false, "Réglement \"v2\".pdf"),
            "attachment; filename=\"R_glement _v2_.pdf\"; \
             filename*=UTF-8''R%C3%A9glement%20%22v2%22.pdf"
        );
    }

    #[test]
    fn if_none_match_matches_the_tag_weakly_or_any() {
        let tag = "\"abc\"";
        let with = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static(value));
            headers
        };
        assert!(none_match(&with("\"abc\""), tag));
        assert!(none_match(&with("W/\"abc\""), tag));
        assert!(none_match(&with("\"x\", \"abc\""), tag));
        assert!(none_match(&with("*"), tag));
        assert!(!none_match(&with("\"abd\""), tag));
        assert!(!none_match(&HeaderMap::new(), tag));
    }
}
