// SPDX-License-Identifier: AGPL-3.0-or-later
//! The shared API conventions (spec 02 B1 to B3, T1, T2) through a router built only for these
//! tests: the routes below use the server's extractors, errors and pages the way product
//! routes do, and are not part of the server.

use axum::{
    Json, Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
    routing::{get, post},
};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use stepclimb_core::id::{Uuid, new_id};
use stepclimb_server::{
    error::{ApiError, FieldError, Problem},
    extract::{ApiJson, ApiPath, ApiQuery},
    pagination::{Page, PageParams},
};
use tower::ServiceExt;

#[derive(Deserialize)]
struct NewThing {
    name: String,
}

/// A create route with one rule: the trimmed name is not empty.
async fn create_thing(ApiJson(body): ApiJson<NewThing>) -> Result<StatusCode, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::invalid(vec![FieldError::at(
            &["name"],
            "name.empty",
        )]));
    }
    Ok(StatusCode::CREATED)
}

async fn get_thing(ApiPath(id): ApiPath<Uuid>) -> String {
    id.to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Item {
    id: Uuid,
    name: String,
}

/// A list sorted by `name, id`, paginated the way a repository does it: filter after the
/// cursor's key, take `fetch_limit` rows, build the page.
async fn list_items(
    items: Vec<Item>,
    ApiQuery(params): ApiQuery<PageParams>,
) -> Result<Json<Page<Item>>, ApiError> {
    let request = params.parse::<(String, Uuid)>()?;
    let mut sorted = items;
    sorted.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
    let fetch = usize::try_from(request.fetch_limit()).expect("a small limit");
    let rows: Vec<Item> = sorted
        .into_iter()
        .filter(|item| {
            request
                .after
                .as_ref()
                .is_none_or(|(name, id)| (&item.name, item.id) > (name, *id))
        })
        .take(fetch)
        .collect();
    Ok(Json(Page::from_fetched(rows, request.limit, |item| {
        (item.name.clone(), item.id)
    })?))
}

/// Seven items; two share a name, so the `id` tiebreak is exercised.
fn seven_items() -> Vec<Item> {
    [
        "Delta", "Alpha", "Golf", "Charlie", "Alpha", "Foxtrot", "Echo",
    ]
    .into_iter()
    .map(|name| Item {
        id: new_id(),
        name: name.to_owned(),
    })
    .collect()
}

fn app(items: Vec<Item>) -> Router {
    Router::new()
        .route("/things", post(create_thing))
        .route("/things/{id}", get(get_thing))
        .route("/items", get(move |query| list_items(items.clone(), query)))
}

struct Answer {
    status: StatusCode,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl Answer {
    fn problem(&self) -> Problem {
        assert_eq!(
            self.content_type.as_deref(),
            Some("application/problem+json"),
            "body: {}",
            String::from_utf8_lossy(&self.body)
        );
        let problem: Problem = serde_json::from_slice(&self.body).expect("a problem body");
        assert_eq!(problem.status, self.status.as_u16());
        assert_eq!(problem.problem_type, "about:blank");
        assert_eq!(Some(problem.title.as_str()), self.status.canonical_reason());
        assert!(!problem.detail.is_empty());
        problem
    }
}

async fn send(app: Router, request: Request<Body>) -> Answer {
    let response = app.oneshot(request).await.expect("the router never fails");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().expect("ASCII").to_owned());
    let body = response
        .into_body()
        .collect()
        .await
        .expect("readable")
        .to_bytes()
        .to_vec();
    Answer {
        status,
        content_type,
        body,
    }
}

fn post_body(content_type: Option<&str>, body: impl Into<Body>) -> Request<Body> {
    let mut request = Request::builder().method(Method::POST).uri("/things");
    if let Some(content_type) = content_type {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    request.body(body.into()).expect("a valid request")
}

fn get_uri(uri: &str) -> Request<Body> {
    Request::get(uri)
        .body(Body::empty())
        .expect("a valid request")
}

#[tokio::test]
async fn a_valid_body_reaches_the_handler() {
    let answer = send(
        app(vec![]),
        post_body(Some("application/json"), r#"{"name":"Tower"}"#),
    )
    .await;
    assert_eq!(answer.status, StatusCode::CREATED);
}

#[tokio::test]
async fn t1_malformed_json_is_400_request_malformed() {
    let answer = send(
        app(vec![]),
        post_body(Some("application/json"), r#"{"name":"#),
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.problem().code, "request.malformed");
}

#[tokio::test]
async fn json_that_does_not_match_the_schema_is_400_request_malformed() {
    for body in [r#"{"name":7}"#, r#"{}"#, r#"[]"#, ""] {
        let answer = send(app(vec![]), post_body(Some("application/json"), body)).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "body {body:?}");
        assert_eq!(answer.problem().code, "request.malformed", "body {body:?}");
    }
}

#[tokio::test]
async fn t1_a_body_of_the_wrong_content_type_is_415() {
    for content_type in [
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
        None,
    ] {
        let answer = send(app(vec![]), post_body(content_type, r#"{"name":"Tower"}"#)).await;
        assert_eq!(
            answer.status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content type {content_type:?}"
        );
        assert_eq!(answer.problem().code, "request.unsupported_media_type");
    }
}

#[tokio::test]
async fn t1_an_empty_name_is_422_with_the_field_pointer() {
    let answer = send(
        app(vec![]),
        post_body(Some("application/json"), r#"{"name":"   "}"#),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNPROCESSABLE_ENTITY);
    let problem = answer.problem();
    assert_eq!(problem.code, "request.invalid");
    assert_eq!(
        problem.errors,
        [FieldError {
            field: "/name".to_owned(),
            code: "name.empty".to_owned()
        }]
    );
}

#[tokio::test]
async fn a_body_over_the_size_limit_is_413() {
    // axum's default limit is 2 MiB.
    let body = format!(r#"{{"name":"{}"}}"#, "a".repeat(3 * 1024 * 1024));
    let answer = send(app(vec![]), post_body(Some("application/json"), body)).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer.problem().code, "request.too_large");
}

#[tokio::test]
async fn a_path_parameter_that_does_not_parse_is_400() {
    let answer = send(app(vec![]), get_uri("/things/not-a-uuid")).await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.problem().code, "request.malformed");

    let id = new_id();
    let ok = send(app(vec![]), get_uri(&format!("/things/{id}"))).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.body, id.to_string().as_bytes());
}

#[tokio::test]
async fn a_query_parameter_that_does_not_parse_is_400() {
    for uri in ["/items?limit=abc", "/items?limit=-1", "/items?limit="] {
        let answer = send(app(seven_items()), get_uri(uri)).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "uri {uri}");
        assert_eq!(answer.problem().code, "request.malformed", "uri {uri}");
    }
}

#[tokio::test]
async fn a_limit_out_of_range_is_400() {
    for uri in ["/items?limit=0", "/items?limit=201"] {
        let answer = send(app(seven_items()), get_uri(uri)).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "uri {uri}");
        assert_eq!(answer.problem().code, "request.malformed", "uri {uri}");
    }
}

async fn page(app: Router, uri: &str) -> Page<Item> {
    #[derive(Deserialize)]
    struct Wire {
        items: Vec<Item>,
        next_cursor: Option<String>,
    }
    let answer = send(app, get_uri(uri)).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.content_type.as_deref(), Some("application/json"));
    let value: serde_json::Value = serde_json::from_slice(&answer.body).expect("JSON");
    assert!(
        value.get("next_cursor").is_some(),
        "next_cursor is present even when null"
    );
    let wire: Wire = serde_json::from_value(value).expect("a page");
    Page {
        items: wire.items,
        next_cursor: wire.next_cursor,
    }
}

#[tokio::test]
async fn t2_seven_items_by_three_come_in_three_pages_in_name_id_order() {
    let items = seven_items();
    let mut expected = items.clone();
    expected.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
    let app = app(items);

    let first = page(app.clone(), "/items?limit=3").await;
    let cursor = first.next_cursor.clone().expect("a second page");
    let second = page(app.clone(), &format!("/items?limit=3&cursor={cursor}")).await;
    let cursor = second.next_cursor.clone().expect("a third page");
    let third = page(app.clone(), &format!("/items?limit=3&cursor={cursor}")).await;

    assert_eq!(
        [first.items.len(), second.items.len(), third.items.len()],
        [3, 3, 1]
    );
    assert_eq!(third.next_cursor, None);
    let all: Vec<Item> = [first.items, second.items, third.items].concat();
    assert_eq!(all, expected);
}

#[tokio::test]
async fn without_a_limit_the_page_holds_up_to_50() {
    let items: Vec<Item> = (0..51)
        .map(|n| Item {
            id: new_id(),
            name: format!("Item {n:02}"),
        })
        .collect();
    let first = page(app(items), "/items").await;
    assert_eq!(first.items.len(), 50);
    assert!(first.next_cursor.is_some());
}

#[tokio::test]
async fn t2_a_tampered_cursor_is_400_pagination_invalid_cursor() {
    let app = app(seven_items());
    let first = page(app.clone(), "/items?limit=3").await;
    let cursor = first.next_cursor.expect("a second page");

    let truncated = &cursor[..cursor.len() - 3];
    let swapped: String = cursor.chars().rev().collect();
    for tampered in [truncated, swapped.as_str(), "not!base64", "e30"] {
        let answer = send(
            app.clone(),
            get_uri(&format!("/items?limit=3&cursor={tampered}")),
        )
        .await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "cursor {tampered}");
        assert_eq!(
            answer.problem().code,
            "pagination.invalid_cursor",
            "cursor {tampered}"
        );
    }
}
