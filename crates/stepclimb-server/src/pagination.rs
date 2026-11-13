// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cursor pagination for every list endpoint (spec 02 B3).
//!
//! A list takes `cursor` and `limit` ([`PageParams`]) and answers a [`Page`]. The cursor is the
//! base64url encoding (no padding) of the JSON of the last item's sort key, such as
//! `["Tower","0190…"]` for a list sorted by `name, id`. It carries no organization: the
//! handler's scope still comes from the credential, so a cursor replayed by another
//! organization's principal only selects that principal's own rows after the key.
//!
//! Clients are told the cursor is opaque, but anyone can decode and edit it: it is not signed or
//! encrypted. A list's sort key may therefore hold only fields that every caller of that list
//! already sees in the item itself, and the repository must treat a decoded key as untrusted
//! input, like any other query parameter.
//!
//! [`PageParams::parse`] is the only way to build a [`PageRequest`]. The repository fetches
//! [`PageRequest::fetch_limit`] rows, one more than the page holds, and [`PageRequest::page`]
//! uses the extra row only to know that another page follows.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use utoipa::{IntoParams, ToSchema};

use crate::error::ApiError;

/// The page size when the request names none (spec 02 Q6).
pub const DEFAULT_LIMIT: u32 = 50;
/// The largest page size a request may ask for (spec 02 Q6).
pub const MAX_LIMIT: u32 = 200;

/// The `detail` of a refused `limit`. A unit test checks that it names [`MAX_LIMIT`].
const LIMIT_DETAIL: &str = "`limit` must be between 1 and 200.";

/// The pagination query parameters of every list endpoint.
//
// The description and `maximum` below repeat DEFAULT_LIMIT and MAX_LIMIT, because doc comments
// and utoipa's attributes take literals; a unit test checks the generated parameters match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageParams {
    /// The `next_cursor` of the previous page; omit it for the first page.
    pub cursor: Option<String>,
    /// How many items to return, 1 to 200; 50 when omitted.
    #[param(minimum = 1, maximum = 200)]
    pub limit: Option<u32>,
}

/// A validated page request: the sort key to start after and the page size. Built only by
/// [`PageParams::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest<K> {
    after: Option<K>,
    limit: u32,
}

impl PageParams {
    /// Validate the limit and decode the cursor into the list's sort key `K`.
    ///
    /// A limit outside 1 to [`MAX_LIMIT`] answers 400 `request.malformed`; a cursor that is not
    /// base64url JSON of a `K` answers 400 `pagination.invalid_cursor`.
    pub fn parse<K: DeserializeOwned>(&self) -> Result<PageRequest<K>, ApiError> {
        let limit = self.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(ApiError::malformed(LIMIT_DETAIL));
        }
        let after = self.cursor.as_deref().map(decode_cursor).transpose()?;
        Ok(PageRequest { after, limit })
    }
}

impl<K> PageRequest<K> {
    /// The sort key of the last item of the previous page; `None` for the first page. It came
    /// from the client: use it only as a bound in a query that is already scoped.
    pub fn after(&self) -> Option<&K> {
        self.after.as_ref()
    }

    /// The page size, 1 to [`MAX_LIMIT`].
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// How many rows the repository fetches: one more than the page, to learn whether another
    /// page follows.
    pub fn fetch_limit(&self) -> i64 {
        i64::from(self.limit) + 1
    }
}

impl<K: Serialize> PageRequest<K> {
    /// The page for `rows`, which the repository fetched in sort order, after [`Self::after`],
    /// with at most [`Self::fetch_limit`] rows. `key` gives an item's sort key; the key of the
    /// page's last item becomes `next_cursor` when the extra row shows another page follows.
    ///
    /// Rows fetched with a smaller limit make every page look like the last one.
    pub fn page<T>(&self, mut rows: Vec<T>, key: impl Fn(&T) -> K) -> Result<Page<T>, ApiError> {
        let page_len = usize::try_from(self.limit).unwrap_or(usize::MAX);
        if rows.len() <= page_len {
            return Ok(Page {
                items: rows,
                next_cursor: None,
            });
        }
        rows.truncate(page_len);
        let next_cursor = rows
            .last()
            .map(|last| encode_cursor(&key(last)))
            .transpose()?;
        Ok(Page {
            items: rows,
            next_cursor,
        })
    }
}

/// One page of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Page<T> {
    /// The items, in the list's sort order.
    pub items: Vec<T>,
    /// Pass this as `cursor` to get the next page; `null` on the last page.
    #[schema(required = true)]
    pub next_cursor: Option<String>,
}

/// The cursor for a sort key.
pub fn encode_cursor<K: Serialize>(key: &K) -> Result<String, ApiError> {
    let json = serde_json::to_vec(key).map_err(|error| ApiError::internal(&error))?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

fn decode_cursor<K: DeserializeOwned>(cursor: &str) -> Result<K, ApiError> {
    let json = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| ApiError::invalid_cursor())?;
    serde_json::from_slice(&json).map_err(|_| ApiError::invalid_cursor())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::code;

    type Key = (String, u32);

    fn params(cursor: Option<&str>, limit: Option<u32>) -> PageParams {
        PageParams {
            cursor: cursor.map(str::to_owned),
            limit,
        }
    }

    #[test]
    fn the_limit_defaults_to_50_and_allows_1_to_200() {
        assert_eq!(
            params(None, None).parse::<Key>().unwrap().limit(),
            DEFAULT_LIMIT
        );
        assert_eq!(params(None, Some(1)).parse::<Key>().unwrap().limit(), 1);
        let max = params(None, Some(MAX_LIMIT)).parse::<Key>().unwrap();
        assert_eq!(max.limit(), MAX_LIMIT);
        for limit in [0, MAX_LIMIT + 1, u32::MAX] {
            let error = params(None, Some(limit)).parse::<Key>().unwrap_err();
            assert_eq!(error.code(), code::REQUEST_MALFORMED, "limit {limit}");
        }
    }

    #[test]
    fn a_cursor_round_trips_its_key() {
        let key: Key = ("Tower".to_owned(), 7);
        let cursor = encode_cursor(&key).unwrap();
        assert!(
            cursor
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "base64url without padding: {cursor}"
        );
        let request = params(Some(&cursor), None).parse::<Key>().unwrap();
        assert_eq!(request.after(), Some(&key));
    }

    #[test]
    fn a_cursor_that_does_not_decode_is_refused() {
        let not_json = URL_SAFE_NO_PAD.encode(b"Tower");
        let wrong_shape = URL_SAFE_NO_PAD.encode(br#"{"name":"Tower"}"#);
        let padded = format!("{}=", encode_cursor(&("a".to_owned(), 1)).unwrap());
        for cursor in [
            "",
            "!!!",
            "a",
            not_json.as_str(),
            wrong_shape.as_str(),
            padded.as_str(),
        ] {
            let error = params(Some(cursor), None).parse::<Key>().unwrap_err();
            assert_eq!(
                error.code(),
                code::PAGINATION_INVALID_CURSOR,
                "cursor {cursor:?}"
            );
        }
    }

    #[test]
    fn a_page_uses_the_extra_row_only_to_set_the_cursor() {
        let request = params(None, Some(3)).parse::<u32>().unwrap();
        let page = request.page(vec![1, 2, 3, 4], |n| *n).unwrap();
        assert_eq!(page.items, [1, 2, 3]);
        assert_eq!(page.next_cursor, Some(encode_cursor(&3).unwrap()));

        let last = request.page(vec![1, 2, 3], |n| *n).unwrap();
        assert_eq!(last.items, [1, 2, 3]);
        assert_eq!(last.next_cursor, None);

        let empty = request.page(Vec::<u32>::new(), |n| *n).unwrap();
        assert!(empty.items.is_empty());
        assert_eq!(empty.next_cursor, None);
    }

    #[test]
    fn the_fetch_limit_is_one_more_than_the_page() {
        let request = params(None, Some(MAX_LIMIT)).parse::<Key>().unwrap();
        assert_eq!(request.fetch_limit(), i64::from(MAX_LIMIT) + 1);
    }

    #[test]
    fn the_limit_detail_and_the_documented_parameter_match_the_constants() {
        assert!(LIMIT_DETAIL.contains(&format!("between 1 and {MAX_LIMIT}")));

        let parameters = <PageParams as IntoParams>::into_params(|| None);
        let limit = parameters
            .iter()
            .find(|p| p.name == "limit")
            .expect("a limit parameter");
        let limit = serde_json::to_value(limit).unwrap();
        assert_eq!(limit["in"], "query");
        assert_eq!(limit["schema"]["minimum"], 1);
        assert_eq!(limit["schema"]["maximum"], MAX_LIMIT);
        let description = limit["description"].as_str().unwrap();
        assert!(
            description.contains(&format!("1 to {MAX_LIMIT}")),
            "{description}"
        );
        assert!(
            description.contains(&format!("{DEFAULT_LIMIT} when omitted")),
            "{description}"
        );
    }

    #[test]
    fn next_cursor_is_always_present_in_the_body_and_the_schema() {
        let json = serde_json::to_value(Page::<u32> {
            items: vec![],
            next_cursor: None,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "items": [], "next_cursor": null })
        );

        let schema = serde_json::to_value(<Page<u32> as utoipa::PartialSchema>::schema()).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&"items".into()));
        assert!(required.contains(&"next_cursor".into()));
    }
}
