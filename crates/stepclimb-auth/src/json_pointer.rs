// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON Pointers (RFC 6901) as provider configs write them: the claim mappings of
//! `docs/specs/m0/05-oidc.md` B13 and `06-oauth2-and-vatsim-preset.md` B6.

use std::fmt;

use serde_json::Value;

/// A syntactically valid JSON Pointer that names a member: it starts with `/`, and every `~` is
/// followed by `0` or `1`. The empty pointer, which RFC 6901 allows and which names the whole
/// document, is refused: a mapping names one claim, never every claim at once.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsonPointer(String);

/// Why a string is not a [`JsonPointer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PointerError {
    #[error("a pointer starts with /")]
    NoLeadingSlash,
    #[error("a ~ in a pointer is followed by 0 or 1")]
    BadEscape,
}

impl JsonPointer {
    /// `text` as a pointer, or why it is not one.
    pub fn parse(text: &str) -> Result<Self, PointerError> {
        if !text.starts_with('/') {
            return Err(PointerError::NoLeadingSlash);
        }
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '~' && !matches!(chars.next(), Some('0' | '1')) {
                return Err(PointerError::BadEscape);
            }
        }
        Ok(Self(text.to_owned()))
    }

    /// The value the pointer names in `document`, or `None` when it names nothing. Each
    /// segment is unescaped as RFC 6901 section 4 says (`~1` to `/`, then `~0` to `~`), names an
    /// object member, or an array element by its decimal index without a leading zero.
    #[must_use]
    pub fn resolve<'a>(&self, document: &'a Value) -> Option<&'a Value> {
        document.pointer(&self.0)
    }

    /// The pointer as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for JsonPointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JsonPointer({:?})", self.0)
    }
}

/// The pointer to the member at `path` (one segment per object key or array index), with `~`
/// and `/` in a segment escaped as RFC 6901 says: `&["claims", "a/b"]` is `/claims/a~1b`.
#[must_use]
pub fn pointer_to(path: &[&str]) -> String {
    path.iter()
        .map(|segment| format!("/{}", segment.replace('~', "~0").replace('/', "~1")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointers_follow_rfc_6901_and_name_a_member() {
        for valid in [
            "/given_name",
            "/data/cid",
            "/a~0b",
            "/a~1b",
            "/",
            "/0",
            "//x",
        ] {
            assert_eq!(
                JsonPointer::parse(valid).map(|p| p.as_str().to_owned()),
                Ok(valid.to_owned()),
                "{valid}"
            );
        }
        assert_eq!(
            JsonPointer::parse("given_name"),
            Err(PointerError::NoLeadingSlash)
        );
        assert_eq!(JsonPointer::parse(""), Err(PointerError::NoLeadingSlash));
        for bad in ["/a~", "/a~2", "/~x/b"] {
            assert_eq!(
                JsonPointer::parse(bad),
                Err(PointerError::BadEscape),
                "{bad}"
            );
        }
    }

    fn resolve(pointer: &str, document: &Value) -> Option<Value> {
        JsonPointer::parse(pointer)
            .expect("a pointer")
            .resolve(document)
            .cloned()
    }

    /// Spec 06 T4's escapes, with RFC 6901 section 5's example document.
    #[test]
    fn spec_t4_escapes_resolve_per_rfc_6901() {
        let document = serde_json::json!({
            "foo": ["bar", "baz"],
            "": 0,
            "a/b": 1,
            "m~n": 8,
            "~1": "tilde-one",
            "data": { "cid": "1000001" },
        });
        assert_eq!(resolve("/a~1b", &document), Some(1.into()));
        assert_eq!(resolve("/m~0n", &document), Some(8.into()));
        // `~01` is `~1`, not `/`: `~1` is replaced before `~0`.
        assert_eq!(resolve("/~01", &document), Some("tilde-one".into()));
        assert_eq!(resolve("/", &document), Some(0.into()));
        assert_eq!(resolve("/foo/0", &document), Some("bar".into()));
        assert_eq!(resolve("/data/cid", &document), Some("1000001".into()));
        for nothing in ["/a/b", "/foo/2", "/foo/01", "/foo/-", "/data/cid/x"] {
            assert_eq!(resolve(nothing, &document), None, "{nothing}");
        }
    }

    #[test]
    fn a_path_becomes_an_escaped_pointer() {
        assert_eq!(pointer_to(&["claims", "given_name"]), "/claims/given_name");
        assert_eq!(pointer_to(&["attributes", "a/b~c"]), "/attributes/a~1b~0c");
        assert_eq!(pointer_to(&[]), "");
    }
}
