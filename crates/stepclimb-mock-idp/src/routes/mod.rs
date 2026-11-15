// SPDX-License-Identifier: AGPL-3.0-or-later
//! The router: every request is recorded, then meets the endpoint's fault, if any, then the
//! endpoint's answer.

mod authorize;
mod discovery;
mod token;
mod userinfo;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use axum::routing::any;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};

use crate::state::{Flavor, Shared};
use crate::types::{Endpoint, Fault, RecordedRequest};

/// The largest request body the mock reads: far above any token request.
const MAX_BODY: usize = 64 * 1024;

/// An endpoint's answer once its request is recorded and no fault replaced it.
type Answer = fn(&Shared, &RecordedRequest) -> Response;

pub(crate) fn router(shared: Shared) -> Router {
    let mut router = Router::new();
    for endpoint in Endpoint::ALL {
        let (methods, answer) = answer_for(endpoint);
        router = router.route(
            endpoint.path(),
            any(move |State(shared): State<Shared>, request: Request| {
                serve(shared, Some(endpoint), methods, answer, request)
            }),
        );
    }
    router
        .fallback(|State(shared): State<Shared>, request: Request| {
            serve(shared, None, &[], not_found, request)
        })
        .with_state(shared)
}

fn answer_for(endpoint: Endpoint) -> (&'static [Method], Answer) {
    const GET: &[Method] = &[Method::GET];
    const POST: &[Method] = &[Method::POST];
    const BOTH: &[Method] = &[Method::GET, Method::POST];
    match endpoint {
        Endpoint::Discovery => (GET, discovery::discovery),
        Endpoint::Jwks => (GET, discovery::jwks),
        Endpoint::Authorize => (GET, |s, r| authorize::answer(s, r, Flavor::Oidc)),
        Endpoint::OAuth2Authorize => (GET, |s, r| authorize::answer(s, r, Flavor::OAuth2)),
        Endpoint::Token => (POST, |s, r| token::answer(s, r, Flavor::Oidc)),
        Endpoint::OAuth2Token => (POST, |s, r| token::answer(s, r, Flavor::OAuth2)),
        Endpoint::Userinfo => (BOTH, |s, r| userinfo::answer(s, r, Flavor::Oidc)),
        Endpoint::OAuth2Userinfo => (BOTH, |s, r| userinfo::answer(s, r, Flavor::OAuth2)),
    }
}

async fn serve(
    shared: Shared,
    endpoint: Option<Endpoint>,
    methods: &'static [Method],
    answer: Answer,
    request: Request,
) -> Response {
    let Some(recorded) = record(endpoint, request).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    shared.lock().requests.push(recorded.clone());
    if endpoint.is_some() && !methods.iter().any(|m| m.as_str() == recorded.method) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let fault = endpoint.and_then(|e| shared.lock().faults.get(&e).cloned());
    if let Some(fault) = fault {
        if let Some(delay) = fault.delay {
            tokio::time::sleep(delay).await;
        }
        if fault.status.is_some() || fault.body.is_some() {
            return fault_response(fault);
        }
    }
    answer(&shared, &recorded)
}

/// The request as a [`RecordedRequest`]; `None` when its body is over [`MAX_BODY`].
async fn record(endpoint: Option<Endpoint>, request: Request) -> Option<RecordedRequest> {
    let (parts, body) = request.into_parts();
    let body = axum::body::to_bytes(body, MAX_BODY).await.ok()?;
    let query = parts.uri.query().map_or_else(Vec::new, |q| {
        url::form_urlencoded::parse(q.as_bytes())
            .into_owned()
            .collect()
    });
    let headers = parts
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    Some(RecordedRequest {
        endpoint,
        method: parts.method.as_str().to_owned(),
        path: parts.uri.path().to_owned(),
        query,
        headers,
        body: body.to_vec(),
    })
}

fn fault_response(fault: Fault) -> Response {
    let status = fault.status.unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status);
    let body = match fault.body {
        Some((content_type, body)) => {
            builder = builder.header(header::CONTENT_TYPE, content_type);
            Body::from(body)
        }
        None => Body::empty(),
    };
    builder
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn not_found(_: &Shared, _: &RecordedRequest) -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// A JSON response with `Cache-Control: no-store`, as RFC 6749 5.1 asks of token responses.
pub(crate) fn json_response(status: StatusCode, body: &Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body.to_string()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// An RFC 6749 5.2 error response.
pub(crate) fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    json_response(
        status,
        &json!({ "error": error, "error_description": description }),
    )
}

/// 32 random bytes in unpadded base64url, for codes and tokens; `None` if the OS RNG fails.
pub(crate) fn random_token() -> Option<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(URL_SAFE_NO_PAD.encode(bytes))
}
