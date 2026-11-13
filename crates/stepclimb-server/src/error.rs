// SPDX-License-Identifier: AGPL-3.0-or-later
//! API errors as RFC 9457 problem details (spec 02 B1, B2).
//!
//! Every error a handler or extractor returns is an [`ApiError`], which answers
//! `application/problem+json` with the members `type`, `title`, `status`, `detail` and a stable
//! `code` (`area.reason`, spec 02 Q4), plus `errors` on a 422 and `limit` and `max` on
//! `entitlement.limit_reached` (spec 10 B10). `type` is `about:blank` and
//! `title` is the HTTP status phrase until the docs site can host a page per code (Q4).
//!
//! `detail` is always a fixed sentence written here or by the handler, never the text of an
//! underlying error: a parser's message can quote the request body, and a database error can
//! carry SQL. A 500 logs its cause once and answers a fixed body.

use axum::{
    extract::rejection::{JsonRejection, PathRejection, QueryRejection},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// The media type of every error body.
pub const PROBLEM_JSON: &str = "application/problem+json";

/// The `type` of every problem until the docs site hosts a page per code (spec 02 Q4).
pub const PROBLEM_TYPE: &str = "about:blank";

/// The codes this module produces. Each area adds its own beside its handlers.
pub mod code {
    /// The body or a parameter does not parse or does not match the schema (400).
    pub const REQUEST_MALFORMED: &str = "request.malformed";
    /// No API route has this path (404).
    pub const REQUEST_NOT_FOUND: &str = "request.not_found";
    /// The route exists but not for this method; `Allow` lists the methods it takes (405).
    pub const REQUEST_METHOD_NOT_ALLOWED: &str = "request.method_not_allowed";
    /// The body is larger than the server accepts (413).
    pub const REQUEST_TOO_LARGE: &str = "request.too_large";
    /// The body is not JSON (415).
    pub const REQUEST_UNSUPPORTED_MEDIA_TYPE: &str = "request.unsupported_media_type";
    /// The body parses but breaks a rule; `errors` says which (422).
    pub const REQUEST_INVALID: &str = "request.invalid";
    /// A pagination cursor that does not decode (400).
    pub const PAGINATION_INVALID_CURSOR: &str = "pagination.invalid_cursor";
    /// Anything the server failed at; the cause is in the log, never the body (500).
    pub const INTERNAL_ERROR: &str = "internal.error";
}

/// One rule a request body broke, in a 422's `errors`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct FieldError {
    /// A JSON Pointer (RFC 6901) to the member that broke the rule, such as `/name`.
    pub field: String,
    /// The stable code of the rule, such as `settings.unknown_key`.
    pub code: String,
}

impl FieldError {
    /// The member at `path` (one segment per object key or array index) broke the rule `code`.
    /// `&["name"]` points at `/name`; `~` and `/` in a segment are escaped as RFC 6901 says.
    pub fn at(path: &[&str], code: &'static str) -> Self {
        let field = path
            .iter()
            .map(|segment| format!("/{}", segment.replace('~', "~0").replace('/', "~1")))
            .collect();
        Self {
            field,
            code: code.to_owned(),
        }
    }
}

/// The body of every error response (RFC 9457).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Problem {
    /// `about:blank` until the docs site documents each code.
    #[serde(rename = "type")]
    pub problem_type: String,
    /// The HTTP status phrase.
    pub title: String,
    /// The HTTP status code, repeated.
    pub status: u16,
    /// A sentence for a human reader. Clients branch on `code`, never on this text.
    pub detail: String,
    /// The stable error code, `area.reason`.
    pub code: String,
    /// On a 422, each rule the body broke.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
    /// On `entitlement.limit_reached`, the key of the limit the request would pass, such as
    /// `users`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    /// On `entitlement.limit_reached`, the plan's value for that limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u64>,
}

/// An error a handler or extractor answers with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    detail: &'static str,
    errors: Vec<FieldError>,
    limit: Option<(&'static str, u64)>,
}

impl ApiError {
    /// An error with any status and code. `detail` must not carry anything the caller could not
    /// otherwise see: no error text from a library, no SQL, no secret, no other organization's id.
    pub fn new(status: StatusCode, code: &'static str, detail: &'static str) -> Self {
        Self {
            status,
            code,
            detail,
            errors: Vec::new(),
            limit: None,
        }
    }

    /// This error with the `limit` and `max` members: the key of the limit the request would
    /// pass and the plan's value for it.
    pub fn with_limit(self, limit: &'static str, max: u64) -> Self {
        Self {
            limit: Some((limit, max)),
            ..self
        }
    }

    /// 400 `request.malformed`.
    pub fn malformed(detail: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code::REQUEST_MALFORMED, detail)
    }

    /// 422 `request.invalid`, listing each rule the body broke.
    pub fn invalid(errors: Vec<FieldError>) -> Self {
        Self {
            errors,
            ..Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                code::REQUEST_INVALID,
                "The request body breaks one or more rules; `errors` lists them.",
            )
        }
    }

    /// 400 `pagination.invalid_cursor`.
    pub fn invalid_cursor() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            code::PAGINATION_INVALID_CURSOR,
            "The cursor does not decode. Start again without a cursor.",
        )
    }

    /// 404 `request.not_found`: no API route has the request's path.
    pub fn route_not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            code::REQUEST_NOT_FOUND,
            "No API operation has this path. The API reference is at /docs.",
        )
    }

    /// 405 `request.method_not_allowed`: the path exists, but not for the request's method.
    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            code::REQUEST_METHOD_NOT_ALLOWED,
            "This path does not take the request's method; the Allow header lists the ones it takes.",
        )
    }

    /// 500 `internal.error`. Logs `cause` once; the body never carries it.
    pub fn internal(cause: &dyn std::error::Error) -> Self {
        tracing::error!(error = %cause, "request failed");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            code::INTERNAL_ERROR,
            "The server could not complete the request.",
        )
    }

    /// The HTTP status.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The stable code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The body this error answers with.
    pub fn problem(&self) -> Problem {
        Problem {
            problem_type: PROBLEM_TYPE.to_owned(),
            // Only unregistered status codes have no phrase, and the server never builds one.
            title: self.status.canonical_reason().unwrap_or("Error").to_owned(),
            status: self.status.as_u16(),
            detail: self.detail.to_owned(),
            code: self.code.to_owned(),
            errors: self.errors.clone(),
            limit: self.limit.map(|(limit, _)| limit.to_owned()),
            max: self.limit.map(|(_, max)| max),
        }
    }

    fn unsupported_media_type() -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            code::REQUEST_UNSUPPORTED_MEDIA_TYPE,
            "The request body must be JSON, sent with `Content-Type: application/json`.",
        )
    }

    fn too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            code::REQUEST_TOO_LARGE,
            "The request body is larger than the server accepts.",
        )
    }

    /// A rejection this module has no specific mapping for, by the status axum gave it: a 5xx
    /// is the server's fault, anything else is the request's.
    fn from_rejection_status(
        status: StatusCode,
        cause: &dyn std::error::Error,
        detail: &'static str,
    ) -> Self {
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            Self::too_large()
        } else if status.is_server_error() {
            Self::internal(cause)
        } else {
            Self::malformed(detail)
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let problem = self.problem();
        // Serializing a struct of strings, a number and a list of such structs cannot fail.
        let body = serde_json::to_vec(&problem).expect("a Problem serializes");
        (
            self.status,
            [(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON))],
            body,
        )
            .into_response()
    }
}

const MALFORMED_BODY: &str = "The request body is not valid JSON for this operation.";

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection {
            JsonRejection::MissingJsonContentType(_) => Self::unsupported_media_type(),
            JsonRejection::JsonSyntaxError(_) | JsonRejection::JsonDataError(_) => {
                Self::malformed(MALFORMED_BODY)
            }
            other => Self::from_rejection_status(other.status(), &other, MALFORMED_BODY),
        }
    }
}

impl From<PathRejection> for ApiError {
    fn from(rejection: PathRejection) -> Self {
        Self::from_rejection_status(
            rejection.status(),
            &rejection,
            "A path parameter is not valid for this operation.",
        )
    }
}

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
        Self::from_rejection_status(
            rejection.status(),
            &rejection,
            "A query parameter is not valid for this operation.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_pointer_escapes_tilde_and_slash() {
        assert_eq!(FieldError::at(&["name"], "x").field, "/name");
        assert_eq!(
            FieldError::at(&["a/b", "m~n", "0"], "x").field,
            "/a~1b/m~0n/0"
        );
        assert_eq!(FieldError::at(&[], "x").field, "");
    }

    #[test]
    fn the_problem_carries_every_member_and_omits_empty_errors() {
        let json = serde_json::to_value(ApiError::malformed("Bad.").problem()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "about:blank",
                "title": "Bad Request",
                "status": 400,
                "detail": "Bad.",
                "code": "request.malformed",
            })
        );
    }

    #[test]
    fn a_limit_error_carries_the_limit_and_its_max() {
        let error = ApiError::new(StatusCode::FORBIDDEN, "entitlement.limit_reached", "Full.")
            .with_limit("users", 2);
        let json = serde_json::to_value(error.problem()).unwrap();
        assert_eq!(json["limit"], "users");
        assert_eq!(json["max"], 2);
        let plain = serde_json::to_value(ApiError::malformed("Bad.").problem()).unwrap();
        assert!(
            plain.get("limit").is_none() && plain.get("max").is_none(),
            "{plain}"
        );
    }

    #[test]
    fn an_invalid_body_lists_its_errors() {
        let error = ApiError::invalid(vec![FieldError::at(&["name"], "name.empty")]);
        let json = serde_json::to_value(error.problem()).unwrap();
        assert_eq!(json["status"], 422);
        assert_eq!(json["title"], "Unprocessable Entity");
        assert_eq!(json["code"], "request.invalid");
        assert_eq!(
            json["errors"],
            serde_json::json!([{ "field": "/name", "code": "name.empty" }])
        );
    }

    #[test]
    fn an_internal_error_does_not_carry_its_cause() {
        let cause = std::io::Error::other("password=hunter2 at db.internal:5432");
        let problem = ApiError::internal(&cause).problem();
        assert_eq!(problem.status, 500);
        assert_eq!(problem.code, "internal.error");
        let body = serde_json::to_string(&problem).unwrap();
        assert!(!body.contains("hunter2"), "the cause leaked: {body}");
        assert!(!body.contains("db.internal"), "the cause leaked: {body}");
    }

    #[test]
    fn rejections_map_by_status_when_no_variant_matches() {
        let cause = std::io::Error::other("boom");
        let too_large = ApiError::from_rejection_status(StatusCode::PAYLOAD_TOO_LARGE, &cause, "d");
        assert_eq!(too_large.code(), code::REQUEST_TOO_LARGE);
        let server =
            ApiError::from_rejection_status(StatusCode::INTERNAL_SERVER_ERROR, &cause, "d");
        assert_eq!(server.code(), code::INTERNAL_ERROR);
        let client = ApiError::from_rejection_status(StatusCode::BAD_REQUEST, &cause, "d");
        assert_eq!(client.code(), code::REQUEST_MALFORMED);
        assert_eq!(client.status(), StatusCode::BAD_REQUEST);
    }
}
