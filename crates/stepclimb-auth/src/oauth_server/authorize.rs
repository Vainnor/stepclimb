// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading an authorization request (docs/specs/m0/08-sessions-and-csrf.md B30, B31; RFC 6749
//! section 4.1, RFC 7636, OAuth 2.1).
//!
//! [`parse`] checks a request's query in this order and stops at the first problem:
//!
//! 1. `client_id` names a client of the registry, given once. Otherwise [`Refusal::Page`]
//!    `oauth.client_unknown`.
//! 2. `redirect_uri` is one of that client's, given once. Otherwise [`Refusal::Page`]
//!    `oauth.redirect_uri_mismatch`. Until both pass, nothing is sent to the redirect URI: an
//!    attacker could name any URI there (RFC 6749 section 4.1.2.1).
//! 3. `state`, when given, is given once, is at most [`MAX_STATE_LEN`] bytes and only RFC 6749's
//!    VSCHAR (`0x20` to `0x7E`). Otherwise [`Refusal::Redirect`] `invalid_request`, without a
//!    `state`, since an unreadable one cannot be returned as it came.
//! 4. No parameter this server reads is given twice (RFC 6749 section 3.1): `invalid_request`.
//! 5. `response_type` is `code`: absent is `invalid_request`, any other value (the implicit
//!    grant's `token`, OIDC's `id_token`) `unsupported_response_type`.
//! 6. `code_challenge_method` is `S256`: absent or `plain` is `invalid_request`.
//! 7. `code_challenge` is an S256 challenge, 43 characters of unpadded base64url holding 32
//!    bytes (RFC 7636 section 4.2): absent or anything else is `invalid_request`.
//!
//! Every other parameter (`scope` among them: the server grants no scopes) is ignored, as RFC
//! 6749 section 3.1 asks.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use url::form_urlencoded;

use super::clients::{self, Client};

/// The longest `state` accepted, in bytes. Without a session the request travels through sign-in
/// in a `return_to` of at most 2,048 bytes (spec 08 B19), percent-encoded at up to three bytes a
/// byte, so the bound keeps the round trip from losing it.
pub const MAX_STATE_LEN: usize = 256;

/// The page codes: refusals shown to the user, never sent to the redirect URI.
pub mod code {
    /// `client_id` is missing, repeated or names no client.
    pub const OAUTH_CLIENT_UNKNOWN: &str = "oauth.client_unknown";
    /// `redirect_uri` is missing, repeated or not one of the client's.
    pub const OAUTH_REDIRECT_URI_MISMATCH: &str = "oauth.redirect_uri_mismatch";
}

/// An authorization request every check passed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub client: &'static Client,
    /// One of the client's redirect URIs, as presented.
    pub redirect_uri: String,
    /// The S256 challenge, as presented.
    pub code_challenge: String,
    /// Returned with the answer exactly as presented, when presented.
    pub state: Option<String>,
}

impl Request {
    /// The query that asks for this request again, with only the parameters this server reads,
    /// in a fixed order: the path back to the authorization endpoint after sign-in.
    #[must_use]
    pub fn to_query(&self) -> String {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", self.client.id)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("code_challenge", &self.code_challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(state) = &self.state {
            query.append_pair("state", state);
        }
        query.finish()
    }

    /// The request as the confirmation step keeps it.
    #[must_use]
    pub fn pending(&self) -> Pending {
        Pending {
            client_id: self.client.id.to_owned(),
            redirect_uri: self.redirect_uri.clone(),
            code_challenge: self.code_challenge.clone(),
            state: self.state.clone(),
        }
    }
}

/// A checked request waiting for the user's answer on the confirmation page, as it is stored
/// (sealed) between the page and the answer. [`Pending::recheck`] checks it against the
/// registry again before a code is issued.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub state: Option<String>,
}

impl Pending {
    /// The stored form.
    #[must_use]
    pub fn to_payload(&self) -> Vec<u8> {
        // A struct of strings always serializes.
        serde_json::to_vec(self).expect("a pending request serializes")
    }

    /// The stored form read back, or `None` when it does not read.
    #[must_use]
    pub fn from_payload(payload: &[u8]) -> Option<Self> {
        serde_json::from_slice(payload).ok()
    }

    /// The request again, when its client still exists and still accepts its redirect URI and
    /// its challenge still reads; `None` otherwise.
    #[must_use]
    pub fn recheck(&self) -> Option<Request> {
        let client = clients::find(&self.client_id)?;
        (client.accepts_redirect(&self.redirect_uri) && is_s256_challenge(&self.code_challenge))
            .then(|| Request {
                client,
                redirect_uri: self.redirect_uri.clone(),
                code_challenge: self.code_challenge.clone(),
                state: self.state.clone(),
            })
    }
}

/// The RFC 6749 section 4.1.2.1 error codes this server sends to a redirect URI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedResponseType,
    /// The user declined on the confirmation page.
    AccessDenied,
}

impl ErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::UnsupportedResponseType => "unsupported_response_type",
            Self::AccessDenied => "access_denied",
        }
    }
}

/// An error answered to the client at its redirect URI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RedirectError {
    /// A redirect URI of the client, checked.
    pub redirect_uri: String,
    pub error: ErrorCode,
    /// A fixed sentence for the developer (`error_description`).
    pub description: &'static str,
    pub state: Option<String>,
}

impl RedirectError {
    /// The redirect URI with `error`, `error_description` and `state`.
    #[must_use]
    pub fn location(&self) -> String {
        let mut pairs = vec![
            ("error", self.error.as_str()),
            ("error_description", self.description),
        ];
        if let Some(state) = &self.state {
            pairs.push(("state", state));
        }
        with_query(&self.redirect_uri, &pairs)
    }
}

/// Why [`parse`] refused a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The client or the redirect URI is wrong: shown to the user with this code, never
    /// redirected.
    Page(&'static str),
    /// Sent to the checked redirect URI.
    Redirect(RedirectError),
}

/// `redirect_uri` with `pairs` as its query. The registered URIs carry no query, so the pairs
/// are the whole of it.
#[must_use]
pub fn with_query(redirect_uri: &str, pairs: &[(&str, &str)]) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        query.append_pair(name, value);
    }
    format!("{redirect_uri}?{}", query.finish())
}

/// The parameters this server reads; each may be given at most once.
const READ: [&str; 6] = [
    "response_type",
    "client_id",
    "redirect_uri",
    "code_challenge",
    "code_challenge_method",
    "state",
];

/// The values of the parameters in [`READ`], in that order: `Err(())` for one given twice.
struct Params(Vec<Result<Option<String>, ()>>);

impl Params {
    fn read(query: &str) -> Self {
        let mut values: Vec<Result<Option<String>, ()>> = vec![Ok(None); READ.len()];
        for (name, value) in form_urlencoded::parse(query.as_bytes()) {
            let Some(index) = READ.iter().position(|read| *read == name) else {
                continue;
            };
            values[index] = match &values[index] {
                Ok(None) => Ok(Some(value.into_owned())),
                _ => Err(()),
            };
        }
        Self(values)
    }

    fn get(&self, name: &str) -> &Result<Option<String>, ()> {
        let index = READ
            .iter()
            .position(|read| *read == name)
            .expect("only parameters of READ are read");
        &self.0[index]
    }

    fn any_repeated(&self) -> bool {
        self.0.iter().any(Result::is_err)
    }
}

/// Check the query of an authorization request (the module docs list the order).
pub fn parse(query: &str) -> Result<Request, Refusal> {
    let params = Params::read(query);
    let client = match params.get("client_id") {
        Ok(Some(id)) => clients::find(id),
        _ => None,
    }
    .ok_or(Refusal::Page(code::OAUTH_CLIENT_UNKNOWN))?;
    let redirect_uri = match params.get("redirect_uri") {
        Ok(Some(uri)) if client.accepts_redirect(uri) => uri.clone(),
        _ => return Err(Refusal::Page(code::OAUTH_REDIRECT_URI_MISMATCH)),
    };
    let refuse = |error, description, state: Option<String>| {
        Err(Refusal::Redirect(RedirectError {
            redirect_uri: redirect_uri.clone(),
            error,
            description,
            state,
        }))
    };
    let state = match params.get("state") {
        Ok(state) if state.as_deref().is_none_or(is_state) => state.clone(),
        _ => return refuse(ErrorCode::InvalidRequest, STATE_INVALID, None),
    };
    if params.any_repeated() {
        return refuse(ErrorCode::InvalidRequest, REPEATED, state);
    }
    match params.get("response_type").as_ref().ok().cloned().flatten() {
        None => return refuse(ErrorCode::InvalidRequest, RESPONSE_TYPE_MISSING, state),
        Some(code) if code == "code" => {}
        Some(_) => return refuse(ErrorCode::UnsupportedResponseType, RESPONSE_TYPE, state),
    }
    let method = params
        .get("code_challenge_method")
        .as_ref()
        .ok()
        .cloned()
        .flatten();
    let challenge = params
        .get("code_challenge")
        .as_ref()
        .ok()
        .cloned()
        .flatten();
    match (method.as_deref(), challenge) {
        (Some("S256"), Some(challenge)) if is_s256_challenge(&challenge) => Ok(Request {
            client,
            redirect_uri,
            code_challenge: challenge,
            state,
        }),
        _ => refuse(ErrorCode::InvalidRequest, PKCE_REQUIRED, state),
    }
}

const STATE_INVALID: &str = "state must be at most 256 printable ASCII characters, given once.";
const REPEATED: &str = "A request parameter is given more than once.";
const RESPONSE_TYPE_MISSING: &str = "response_type is required.";
const RESPONSE_TYPE: &str = "Only response_type=code is supported.";
const PKCE_REQUIRED: &str =
    "PKCE is required: code_challenge_method=S256 and a code_challenge of 43 base64url characters.";

/// RFC 6749 appendix A.5: `state` is VSCHAR, and this server bounds its length.
fn is_state(state: &str) -> bool {
    state.len() <= MAX_STATE_LEN && state.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

/// An S256 `code_challenge`: BASE64URL(SHA-256(verifier)) without padding (RFC 7636 section
/// 4.2), so exactly 43 characters that decode to 32 bytes.
#[must_use]
pub fn is_s256_challenge(challenge: &str) -> bool {
    challenge.len() == 43
        && URL_SAFE_NO_PAD
            .decode(challenge)
            .is_ok_and(|bytes| bytes.len() == 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636 appendix B's challenge.
    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    const REDIRECT: &str = "dev.stepclimb.shell:/oauth/callback";

    fn query(pairs: &[(&str, &str)]) -> String {
        let mut query = form_urlencoded::Serializer::new(String::new());
        for (name, value) in pairs {
            query.append_pair(name, value);
        }
        query.finish()
    }

    fn good() -> Vec<(&'static str, &'static str)> {
        vec![
            ("response_type", "code"),
            ("client_id", "stepclimb-app"),
            ("redirect_uri", REDIRECT),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("state", "xyz 1"),
        ]
    }

    fn with(name: &'static str, value: Option<&'static str>) -> String {
        let mut pairs: Vec<_> = good().into_iter().filter(|(n, _)| *n != name).collect();
        pairs.extend(value.map(|value| (name, value)));
        query(&pairs)
    }

    fn redirected(query: &str) -> RedirectError {
        match parse(query) {
            Err(Refusal::Redirect(error)) => error,
            other => panic!("{query}: {other:?}"),
        }
    }

    #[test]
    fn a_good_request_passes_and_keeps_its_values() {
        let request = parse(&query(&good())).expect("a good request");
        assert_eq!(request.client.id, "stepclimb-app");
        assert_eq!(request.redirect_uri, REDIRECT);
        assert_eq!(request.code_challenge, CHALLENGE);
        assert_eq!(request.state.as_deref(), Some("xyz 1"));
        assert_eq!(parse(&with("state", None)).expect("no state").state, None);
        let extra = format!("{}&scope=openid&nonce=1&foo=bar", query(&good()));
        assert_eq!(parse(&extra), Ok(request));
    }

    #[test]
    fn a_wrong_client_or_redirect_is_shown_and_never_redirected() {
        for query in [
            with("client_id", None),
            with("client_id", Some("other-app")),
            format!("{}&client_id=stepclimb-app", query(&good())),
        ] {
            assert_eq!(
                parse(&query),
                Err(Refusal::Page(code::OAUTH_CLIENT_UNKNOWN)),
                "{query}"
            );
        }
        for query in [
            with("redirect_uri", None),
            with("redirect_uri", Some("dev.stepclimb.shell:/oauth/callbacks")),
            with("redirect_uri", Some("https://evil.example/cb")),
            format!("{}&redirect_uri={REDIRECT}", query(&good())),
        ] {
            assert_eq!(
                parse(&query),
                Err(Refusal::Page(code::OAUTH_REDIRECT_URI_MISMATCH)),
                "{query}"
            );
        }
    }

    #[test]
    fn the_client_is_checked_before_everything_else() {
        let query = query(&[("client_id", "nobody"), ("response_type", "token")]);
        assert_eq!(
            parse(&query),
            Err(Refusal::Page(code::OAUTH_CLIENT_UNKNOWN))
        );
        let query = query_with_redirect("https://evil.example/");
        assert_eq!(
            parse(&query),
            Err(Refusal::Page(code::OAUTH_REDIRECT_URI_MISMATCH))
        );
    }

    fn query_with_redirect(redirect: &'static str) -> String {
        query(&[
            ("client_id", "stepclimb-app"),
            ("redirect_uri", redirect),
            ("response_type", "token"),
        ])
    }

    #[test]
    fn t27_pkce_s256_is_required() {
        for query in [
            with("code_challenge", None),
            with("code_challenge_method", None),
            with("code_challenge_method", Some("plain")),
            with("code_challenge_method", Some("s256")),
            with("code_challenge", Some("short")),
            with(
                "code_challenge",
                Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM="),
            ),
            with(
                "code_challenge",
                Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw+cM"),
            ),
            with(
                "code_challenge",
                Some("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cMx"),
            ),
        ] {
            let error = redirected(&query);
            assert_eq!(error.error, ErrorCode::InvalidRequest, "{query}");
            assert_eq!(error.state.as_deref(), Some("xyz 1"), "{query}");
        }
    }

    #[test]
    fn t27_only_the_code_response_type_passes() {
        for value in ["token", "id_token", "code token", "CODE"] {
            let error = redirected(&with("response_type", Some(value)));
            assert_eq!(error.error, ErrorCode::UnsupportedResponseType, "{value}");
        }
        assert_eq!(
            redirected(&with("response_type", None)).error,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn a_repeated_parameter_is_an_invalid_request() {
        for name in ["response_type", "code_challenge", "code_challenge_method"] {
            let query = format!("{}&{name}=code", query(&good()));
            let error = redirected(&query);
            assert_eq!(error.error, ErrorCode::InvalidRequest, "{name}");
            assert_eq!(error.state.as_deref(), Some("xyz 1"));
        }
        let error = redirected(&format!("{}&state=again", query(&good())));
        assert_eq!(
            (error.error, error.state),
            (ErrorCode::InvalidRequest, None)
        );
    }

    #[test]
    fn a_state_that_cannot_be_returned_is_refused_without_it() {
        let long: &'static str = Box::leak("s".repeat(MAX_STATE_LEN + 1).into_boxed_str());
        for state in ["tab\there", "caf\u{e9}", long] {
            let error = redirected(&with("state", Some(state)));
            assert_eq!(
                (error.error, error.state),
                (ErrorCode::InvalidRequest, None)
            );
        }
        let longest: &'static str = Box::leak("~".repeat(MAX_STATE_LEN).into_boxed_str());
        assert!(parse(&with("state", Some(longest))).is_ok());
    }

    #[test]
    fn an_error_location_carries_the_error_and_the_state() {
        let error = redirected(&with("response_type", Some("token")));
        assert_eq!(
            error.location(),
            "dev.stepclimb.shell:/oauth/callback?error=unsupported_response_type\
             &error_description=Only+response_type%3Dcode+is+supported.&state=xyz+1"
        );
    }

    #[test]
    fn the_query_back_reads_as_the_same_request() {
        let request = parse(&query(&good())).expect("a good request");
        assert_eq!(parse(&request.to_query()), Ok(request.clone()));
        let without = Request {
            state: None,
            ..request
        };
        assert_eq!(parse(&without.to_query()), Ok(without));
    }

    #[test]
    fn a_pending_request_rechecks_against_the_registry() {
        let request = parse(&query(&good())).expect("a good request");
        let pending = request.pending();
        let read = Pending::from_payload(&pending.to_payload()).expect("reads back");
        assert_eq!(read.recheck(), Some(request));
        for changed in [
            Pending {
                client_id: "gone".into(),
                ..pending.clone()
            },
            Pending {
                redirect_uri: "https://evil.example/".into(),
                ..pending.clone()
            },
            Pending {
                code_challenge: "plain-verifier".into(),
                ..pending.clone()
            },
        ] {
            assert_eq!(changed.recheck(), None, "{changed:?}");
        }
        assert_eq!(Pending::from_payload(b"{\"client_id\":1}"), None);
    }
}
