// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading one named cookie from a request, for the session and sign-in flow cookies.

use axum::http::{HeaderMap, header};

/// What the request's `Cookie` headers say about one cookie.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Presented<'a> {
    Absent,
    One(&'a str),
    /// One cookie of that name whose value is not ASCII, which no token is.
    NotAscii,
    /// More than one cookie of that name, which a browser never sends for one `__Host-` name;
    /// the request is not trusted to mean either.
    Ambiguous,
}

/// The value of every cookie named `name` in the request's `Cookie` headers, `None` for one
/// that is not ASCII.
///
/// The headers are split as bytes, so a neighbor cookie with a non-ASCII value (which
/// `HeaderValue::to_str` refuses for the whole header) does not hide the named cookie.
pub(crate) fn values<'h>(headers: &'h HeaderMap, name: &str) -> Vec<Option<&'h str>> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .flat_map(|line| line.as_bytes().split(|&byte| byte == b';'))
        .filter_map(|pair| {
            let pair = pair.trim_ascii();
            let equals = pair.iter().position(|&byte| byte == b'=')?;
            let (found, value) = (&pair[..equals], &pair[equals + 1..]);
            (found == name.as_bytes())
                .then(|| std::str::from_utf8(value).ok().filter(|v| v.is_ascii()))
        })
        .collect()
}

/// The one cookie named `name`, if the request carries exactly one with an ASCII value.
pub(crate) fn presented<'h>(headers: &'h HeaderMap, name: &str) -> Presented<'h> {
    match values(headers, name).as_slice() {
        [] => Presented::Absent,
        [Some(value)] => Presented::One(value),
        [None] => Presented::NotAscii,
        _ => Presented::Ambiguous,
    }
}
