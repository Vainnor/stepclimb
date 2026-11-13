// SPDX-License-Identifier: AGPL-3.0-or-later
//! A web origin (`scheme://host[:port]`), as browsers send it in the `Origin` header.
//!
//! The instance's public origin (`STEPCLIMB_PUBLIC_URL`, spec 08 Q1) and the extra origins the
//! CSRF check accepts (`STEPCLIMB_ALLOWED_ORIGINS`, Q6) are parsed into this type, so the check
//! compares the header's bytes with the same serialization a browser produces: the scheme and host
//! in lower case, an internationalized host in punycode, and the scheme's default port left out.

use std::fmt;

/// An `http` or `https` origin with no path, query, fragment or credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct WebOrigin {
    serialized: String,
    https: bool,
}

/// Why a value is not a web origin. The message is fixed text: configuration errors add the
/// setting's name, and the value itself is not repeated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WebOriginError {
    #[error("is not a URL")]
    NotAUrl,
    #[error("must start with http:// or https://")]
    Scheme,
    #[error("must not carry a user name or password")]
    Credentials,
    #[error("must be an origin only, with no path, query or fragment")]
    NotAnOrigin,
}

impl WebOrigin {
    /// Parse `value`, such as `https://lms.example.org` (a trailing `/` is accepted).
    pub fn parse(value: &str) -> Result<Self, WebOriginError> {
        let url = url::Url::parse(value).map_err(|_| WebOriginError::NotAUrl)?;
        let https = match url.scheme() {
            "https" => true,
            "http" => false,
            _ => return Err(WebOriginError::Scheme),
        };
        if !url.username().is_empty() || url.password().is_some() {
            return Err(WebOriginError::Credentials);
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(WebOriginError::NotAnOrigin);
        }
        Ok(Self {
            serialized: url.origin().ascii_serialization(),
            https,
        })
    }

    /// The origin as a browser writes it in the `Origin` header.
    pub fn as_str(&self) -> &str {
        &self.serialized
    }

    /// Whether the scheme is `https`.
    pub fn is_https(&self) -> bool {
        self.https
    }
}

impl fmt::Display for WebOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialized)
    }
}

impl fmt::Debug for WebOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebOrigin({:?})", self.serialized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_origin_is_serialized_as_a_browser_sends_it() {
        for (value, expected, https) in [
            ("https://lms.example.org", "https://lms.example.org", true),
            ("https://LMS.Example.org/", "https://lms.example.org", true),
            (
                "https://lms.example.org:443",
                "https://lms.example.org",
                true,
            ),
            ("http://localhost:8080", "http://localhost:8080", false),
            ("http://127.0.0.1:80/", "http://127.0.0.1", false),
            (
                "https://bücher.example",
                "https://xn--bcher-kva.example",
                true,
            ),
        ] {
            let origin = WebOrigin::parse(value).unwrap();
            assert_eq!(origin.as_str(), expected, "{value}");
            assert_eq!(origin.is_https(), https, "{value}");
        }
    }

    #[test]
    fn anything_but_a_bare_http_or_https_origin_is_refused() {
        for (value, error) in [
            ("lms.example.org", WebOriginError::NotAUrl),
            ("", WebOriginError::NotAUrl),
            ("ftp://lms.example.org", WebOriginError::Scheme),
            ("file:///etc/passwd", WebOriginError::Scheme),
            (
                "https://user:pw@lms.example.org",
                WebOriginError::Credentials,
            ),
            ("https://user@lms.example.org", WebOriginError::Credentials),
            ("https://lms.example.org/lms", WebOriginError::NotAnOrigin),
            ("https://lms.example.org/?a=1", WebOriginError::NotAnOrigin),
            ("https://lms.example.org/#top", WebOriginError::NotAnOrigin),
        ] {
            assert_eq!(WebOrigin::parse(value), Err(error), "{value}");
        }
    }
}
