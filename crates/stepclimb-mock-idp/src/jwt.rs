// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compact JWS encoding of ID tokens (RFC 7515 7.1), the `at_hash` claim (OIDC Core 3.1.3.6) and
//! the PKCE S256 transform (RFC 7636 4.2).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Map, Value, json};
use sha2::{Digest as _, Sha256};

use crate::keys::{Key, SignError};

/// The signed (or, for `alg: none`, unsigned) compact form of `claims`.
pub(crate) fn encode(
    claims: &Map<String, Value>,
    signer: Option<&Key>,
) -> Result<String, SignError> {
    let header = match signer {
        Some(key) => json!({ "alg": "RS256", "typ": "JWT", "kid": key.kid() }),
        None => json!({ "alg": "none", "typ": "JWT" }),
    };
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(Value::Object(claims.clone()).to_string()),
    );
    let signature = match signer {
        Some(key) => URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes())?),
        None => String::new(),
    };
    Ok(format!("{input}.{signature}"))
}

/// `at_hash` for an RS256 token: the left half of the SHA-256 of the access token, base64url.
pub(crate) fn at_hash(access_token: &str) -> String {
    let digest = Sha256::digest(access_token.as_bytes());
    URL_SAFE_NO_PAD.encode(&digest[..digest.len() / 2])
}

/// The S256 code challenge of `verifier`.
pub(crate) fn s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Whether `verifier` has the form RFC 7636 4.1 requires: 43 to 128 unreserved characters.
pub(crate) fn verifier_well_formed(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s256_matches_the_rfc_7636_appendix_b_example() {
        assert_eq!(
            s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn at_hash_matches_the_oidc_core_a_3_example() {
        // OpenID Connect Core 1.0, Appendix A.3: the access token and the at_hash it lists.
        assert_eq!(
            at_hash("jHkWEdUXMU1BwAsC4vtUsZwnNvTIxEl0z9K3vx5KF0Y"),
            "77QmUPtjPfzWtF2AnpK9RQ"
        );
    }

    #[test]
    fn verifier_form_follows_rfc_7636() {
        assert!(verifier_well_formed(&"a".repeat(43)));
        assert!(verifier_well_formed(&"-._~".repeat(32)));
        assert!(!verifier_well_formed(&"a".repeat(42)));
        assert!(!verifier_well_formed(&"a".repeat(129)));
        assert!(!verifier_well_formed(&format!("{}+", "a".repeat(42))));
    }

    #[test]
    fn an_unsigned_token_has_alg_none_and_an_empty_signature() {
        let mut claims = Map::new();
        claims.insert("sub".to_owned(), json!("s"));
        let token = encode(&claims, None).expect("no signing");
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[2], "");
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).expect("b64")).expect("json");
        assert_eq!(header, json!({ "alg": "none", "typ": "JWT" }));
    }
}
