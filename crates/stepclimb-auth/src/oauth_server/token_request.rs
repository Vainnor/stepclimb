// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reading token and revocation requests (docs/specs/m0/08-sessions-and-csrf.md B30, B32 to B35,
//! B37, Q13; RFC 6749 sections 3.2, 4.1.3, 5.2 and 6, RFC 7636 section 4.5, RFC 7009 section
//! 2.1).
//!
//! Both bodies are `application/x-www-form-urlencoded`, and no parameter this server reads may be
//! given twice (RFC 6749 section 3.2). [`parse_token`] checks a token request in this order and
//! stops at the first problem, before anything is looked up:
//!
//! 1. a parameter this server reads is given twice: `invalid_request`;
//! 2. `grant_type` is `authorization_code` or `refresh_token`: absent is `invalid_request`, any
//!    other value (`password`, `client_credentials`, `implicit`) `unsupported_grant_type`;
//! 3. the grant's parameters are present and not empty: `code`, `redirect_uri`, `client_id` and
//!    `code_verifier`, or `refresh_token` and `client_id`. Otherwise `invalid_request`;
//! 4. `code_verifier` is RFC 7636's: 43 to 128 characters of `A-Z a-z 0-9 - . _ ~`. Otherwise
//!    `invalid_request`;
//! 5. `client_id` names a client of the registry: otherwise `invalid_client` (401).
//!
//! Other parameters (`scope`, `client_secret`) are ignored (RFC 6749 section 3.2). Whether the
//! code or refresh token is good is the server's to decide, against what it stored, and answers
//! `invalid_grant`. [`parse_revoke`] reads `token`, `token_type_hint` and `client_id` the same
//! way. [`verifier_matches`] is the S256 check of RFC 7636 section 4.6.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::form_urlencoded;

use super::clients::{self, Client};

/// The longest body either endpoint reads, in bytes. A refresh request is under 200 bytes and a
/// code exchange under 400.
pub const MAX_BODY_LEN: usize = 4096;

/// The RFC 6749 section 5.2 error codes these endpoints answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidRequest,
    /// The `client_id` names no client (401).
    InvalidClient,
    /// The code or refresh token is unknown, expired, used, revoked, another client's, bound to
    /// another redirect URI or challenge, or its user is suspended.
    InvalidGrant,
    UnsupportedGrantType,
}

impl ErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::UnsupportedGrantType => "unsupported_grant_type",
        }
    }
}

/// A refused request: the code and a fixed sentence for the developer (`error_description`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub error: ErrorCode,
    pub description: &'static str,
}

impl Refusal {
    const fn new(error: ErrorCode, description: &'static str) -> Self {
        Self { error, description }
    }
}

/// The refusal of a code or refresh token that does not pass, whatever the reason: the answer
/// never says which check failed.
pub const INVALID_GRANT: Refusal = Refusal::new(
    ErrorCode::InvalidGrant,
    "The authorization code or refresh token is not valid. Sign in again.",
);

const REPEATED: Refusal = Refusal::new(
    ErrorCode::InvalidRequest,
    "A request parameter is given more than once.",
);
const GRANT_TYPE_MISSING: Refusal =
    Refusal::new(ErrorCode::InvalidRequest, "grant_type is required.");
const GRANT_TYPE: Refusal = Refusal::new(
    ErrorCode::UnsupportedGrantType,
    "Only the authorization_code and refresh_token grants are supported.",
);
const CODE_PARAMS: Refusal = Refusal::new(
    ErrorCode::InvalidRequest,
    "The authorization_code grant needs code, redirect_uri, client_id and code_verifier.",
);
const REFRESH_PARAMS: Refusal = Refusal::new(
    ErrorCode::InvalidRequest,
    "The refresh_token grant needs refresh_token and client_id.",
);
const VERIFIER: Refusal = Refusal::new(
    ErrorCode::InvalidRequest,
    "code_verifier must be 43 to 128 characters of A-Z, a-z, 0-9, '-', '.', '_' and '~'.",
);
const CLIENT: Refusal = Refusal::new(ErrorCode::InvalidClient, "client_id names no client.");
const REVOKE_PARAMS: Refusal = Refusal::new(
    ErrorCode::InvalidRequest,
    "A revocation request needs token and client_id.",
);

/// A token request every format check passed.
#[derive(Debug, PartialEq, Eq)]
pub enum TokenRequest {
    /// `grant_type=authorization_code`.
    AuthorizationCode {
        code: String,
        redirect_uri: String,
        client: &'static Client,
        code_verifier: String,
    },
    /// `grant_type=refresh_token`.
    RefreshToken {
        refresh_token: String,
        client: &'static Client,
    },
}

/// A revocation request every format check passed. `token_type_hint` is read only so it is
/// given at most once; the server tries both kinds of token whatever it says (RFC 7009 section
/// 2.1 lets it).
#[derive(Debug, PartialEq, Eq)]
pub struct RevokeRequest {
    pub token: String,
    pub client: &'static Client,
}

/// The values of the parameters a request reads, by name, each at most once.
struct Form<'n> {
    names: &'n [&'n str],
    values: Vec<Option<String>>,
}

impl<'n> Form<'n> {
    /// Read `body`, refusing a parameter of `names` given twice.
    fn read(body: &[u8], names: &'n [&'n str]) -> Result<Self, Refusal> {
        let mut values = vec![None; names.len()];
        for (name, value) in form_urlencoded::parse(body) {
            let Some(index) = names.iter().position(|read| *read == name) else {
                continue;
            };
            if values[index].replace(value.into_owned()).is_some() {
                return Err(REPEATED);
            }
        }
        Ok(Self { names, values })
    }

    /// The value of `name`, when it is given and not empty (RFC 6749 section 3.1: a parameter
    /// sent without a value is treated as omitted).
    fn take(&mut self, name: &str) -> Option<String> {
        let index = self
            .names
            .iter()
            .position(|read| *read == name)
            .expect("only declared parameters are taken");
        self.values[index].take().filter(|value| !value.is_empty())
    }
}

const TOKEN_PARAMS: [&str; 6] = [
    "grant_type",
    "code",
    "redirect_uri",
    "client_id",
    "code_verifier",
    "refresh_token",
];

/// Check a token request's body (the module docs list the order).
pub fn parse_token(body: &[u8]) -> Result<TokenRequest, Refusal> {
    let mut form = Form::read(body, &TOKEN_PARAMS)?;
    match form.take("grant_type").as_deref() {
        None => Err(GRANT_TYPE_MISSING),
        Some("authorization_code") => {
            let (Some(code), Some(redirect_uri), Some(client_id), Some(code_verifier)) = (
                form.take("code"),
                form.take("redirect_uri"),
                form.take("client_id"),
                form.take("code_verifier"),
            ) else {
                return Err(CODE_PARAMS);
            };
            if !is_verifier(&code_verifier) {
                return Err(VERIFIER);
            }
            Ok(TokenRequest::AuthorizationCode {
                code,
                redirect_uri,
                client: client(&client_id)?,
                code_verifier,
            })
        }
        Some("refresh_token") => {
            let (Some(refresh_token), Some(client_id)) =
                (form.take("refresh_token"), form.take("client_id"))
            else {
                return Err(REFRESH_PARAMS);
            };
            Ok(TokenRequest::RefreshToken {
                refresh_token,
                client: client(&client_id)?,
            })
        }
        Some(_) => Err(GRANT_TYPE),
    }
}

/// Check a revocation request's body: `token` and `client_id` once each, `token_type_hint` at
/// most once.
pub fn parse_revoke(body: &[u8]) -> Result<RevokeRequest, Refusal> {
    let mut form = Form::read(body, &["token", "token_type_hint", "client_id"])?;
    let (Some(token), Some(client_id)) = (form.take("token"), form.take("client_id")) else {
        return Err(REVOKE_PARAMS);
    };
    Ok(RevokeRequest {
        token,
        client: client(&client_id)?,
    })
}

fn client(id: &str) -> Result<&'static Client, Refusal> {
    clients::find(id).ok_or(CLIENT)
}

/// RFC 7636 section 4.1: `code-verifier = 43*128unreserved`.
fn is_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

/// Whether `BASE64URL(SHA-256(code_verifier))` is `challenge` (RFC 7636 section 4.6), compared in
/// constant time.
#[must_use]
pub fn verifier_matches(code_verifier: &str, challenge: &str) -> bool {
    let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    computed.as_bytes().ct_eq(challenge.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth_server::clients::STEPCLIMB_APP;

    /// RFC 7636 appendix B.
    const VERIFIER_B: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const CHALLENGE_B: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
    const REDIRECT: &str = "dev.stepclimb.shell:/oauth/callback";

    fn body(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut form = form_urlencoded::Serializer::new(String::new());
        for (name, value) in pairs {
            form.append_pair(name, value);
        }
        form.finish().into_bytes()
    }

    fn code_pairs() -> Vec<(&'static str, &'static str)> {
        vec![
            ("grant_type", "authorization_code"),
            ("code", "the-code"),
            ("redirect_uri", REDIRECT),
            ("client_id", "stepclimb-app"),
            ("code_verifier", VERIFIER_B),
        ]
    }

    fn without(pairs: &[(&'static str, &'static str)], name: &str) -> Vec<u8> {
        let kept: Vec<_> = pairs.iter().copied().filter(|(n, _)| *n != name).collect();
        body(&kept)
    }

    fn with(name: &'static str, value: &'static str) -> Vec<u8> {
        let mut pairs: Vec<_> = code_pairs()
            .into_iter()
            .filter(|(n, _)| *n != name)
            .collect();
        pairs.push((name, value));
        body(&pairs)
    }

    fn error(body: &[u8]) -> ErrorCode {
        parse_token(body).expect_err("refused").error
    }

    #[test]
    fn the_rfc_7636_verifier_matches_its_challenge_and_nothing_else() {
        assert!(verifier_matches(VERIFIER_B, CHALLENGE_B));
        assert!(!verifier_matches(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXK",
            CHALLENGE_B
        ));
        assert!(!verifier_matches(VERIFIER_B, &CHALLENGE_B[..42]));
        assert!(!verifier_matches(CHALLENGE_B, CHALLENGE_B));
    }

    #[test]
    fn a_code_exchange_reads_its_four_parameters() {
        let extra = [code_pairs(), vec![("scope", "x"), ("client_secret", "s")]].concat();
        assert_eq!(
            parse_token(&body(&extra)),
            Ok(TokenRequest::AuthorizationCode {
                code: "the-code".into(),
                redirect_uri: REDIRECT.into(),
                client: &STEPCLIMB_APP,
                code_verifier: VERIFIER_B.into(),
            })
        );
    }

    #[test]
    fn a_refresh_reads_its_token_and_client() {
        let request = body(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", "r1"),
            ("client_id", "stepclimb-app"),
            ("scope", "ignored"),
        ]);
        assert_eq!(
            parse_token(&request),
            Ok(TokenRequest::RefreshToken {
                refresh_token: "r1".into(),
                client: &STEPCLIMB_APP,
            })
        );
        for name in ["refresh_token", "client_id"] {
            let pairs = [
                ("grant_type", "refresh_token"),
                ("refresh_token", "r1"),
                ("client_id", "stepclimb-app"),
            ];
            assert_eq!(
                error(&without(&pairs, name)),
                ErrorCode::InvalidRequest,
                "{name}"
            );
        }
    }

    #[test]
    fn t27_other_grants_are_unsupported_and_a_missing_one_is_invalid() {
        for grant in [
            "password",
            "client_credentials",
            "implicit",
            "token",
            "AUTHORIZATION_CODE",
        ] {
            let request = body(&[("grant_type", grant), ("username", "u"), ("password", "p")]);
            assert_eq!(error(&request), ErrorCode::UnsupportedGrantType, "{grant}");
        }
        assert_eq!(error(b""), ErrorCode::InvalidRequest);
        assert_eq!(error(b"grant_type="), ErrorCode::InvalidRequest);
        assert_eq!(error(b"\xff\xfe"), ErrorCode::InvalidRequest);
    }

    #[test]
    fn every_code_parameter_is_required() {
        for name in ["code", "redirect_uri", "client_id", "code_verifier"] {
            assert_eq!(
                error(&without(&code_pairs(), name)),
                ErrorCode::InvalidRequest,
                "{name}"
            );
            assert_eq!(error(&with(name, "")), ErrorCode::InvalidRequest, "{name}");
        }
    }

    #[test]
    fn a_repeated_parameter_is_invalid_even_when_both_agree() {
        for name in TOKEN_PARAMS {
            let mut pairs = code_pairs();
            pairs.push((name, "x"));
            if !code_pairs().iter().any(|(n, _)| *n == name) {
                pairs.push((name, "x"));
            }
            assert_eq!(error(&body(&pairs)), ErrorCode::InvalidRequest, "{name}");
        }
        let same = [code_pairs(), vec![("client_id", "stepclimb-app")]].concat();
        assert_eq!(error(&body(&same)), ErrorCode::InvalidRequest);
    }

    #[test]
    fn the_verifier_is_checked_for_its_format() {
        let long: &'static str = Box::leak("a".repeat(129).into_boxed_str());
        let longest: &'static str = Box::leak("~".repeat(128).into_boxed_str());
        for bad in [
            "short",
            &VERIFIER_B[..42],
            long,
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjX+",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjX=",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEj X",
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEj\u{e9}",
        ] {
            let error = parse_token(&with("code_verifier", bad)).expect_err(bad);
            assert_eq!(error, VERIFIER, "{bad}");
        }
        assert!(parse_token(&with("code_verifier", longest)).is_ok());
        assert!(parse_token(&with("code_verifier", &VERIFIER_B[..43])).is_ok());
    }

    #[test]
    fn an_unknown_client_is_invalid_client_after_the_format_checks() {
        assert_eq!(
            error(&with("client_id", "other-app")),
            ErrorCode::InvalidClient
        );
        let both = body(&[
            ("grant_type", "authorization_code"),
            ("code", "c"),
            ("redirect_uri", REDIRECT),
            ("client_id", "other-app"),
            ("code_verifier", "short"),
        ]);
        assert_eq!(error(&both), ErrorCode::InvalidRequest);
    }

    #[test]
    fn a_revocation_needs_its_token_and_client() {
        let good = [
            ("token", "t"),
            ("token_type_hint", "refresh_token"),
            ("client_id", "stepclimb-app"),
        ];
        assert_eq!(
            parse_revoke(&body(&good)),
            Ok(RevokeRequest {
                token: "t".into(),
                client: &STEPCLIMB_APP,
            })
        );
        assert!(parse_revoke(&without(&good, "token_type_hint")).is_ok());
        for name in ["token", "client_id"] {
            assert_eq!(
                parse_revoke(&without(&good, name)).unwrap_err().error,
                ErrorCode::InvalidRequest
            );
        }
        let twice = [good.to_vec(), vec![("token_type_hint", "access_token")]].concat();
        assert_eq!(
            parse_revoke(&body(&twice)).unwrap_err().error,
            ErrorCode::InvalidRequest
        );
        let other = body(&[("token", "t"), ("client_id", "other")]);
        assert_eq!(
            parse_revoke(&other).unwrap_err().error,
            ErrorCode::InvalidClient
        );
    }
}
