// SPDX-License-Identifier: AGPL-3.0-or-later
//! The signing secret of a Standard Webhooks endpoint and the signature it makes
//! (docs/specs/m0/07-admission.md B13, B24, Q7 (a)): the admission webhook's today.
//!
//! A [`Secret`] is [`SECRET_BYTES`] random bytes from the operating system's CSPRNG, inside the
//! 24 to 64 bytes the Standard Webhooks specification asks for an HMAC-SHA256 key. A receiver is
//! given it once as `whsec_` followed by the bytes in standard padded base64
//! ([`Secret::expose_whsec`]), the form the Standard Webhooks libraries read.
//!
//! A request is signed as Standard Webhooks says ([`Secret::sign`]): HMAC-SHA256, keyed with the
//! secret's bytes, over the message id, the Unix timestamp in seconds and the body, joined by
//! `.`, sent in the `webhook-signature` header as `v1,` and the MAC in standard padded base64,
//! beside the `webhook-id` and `webhook-timestamp` headers that carry the first two.
//!
//! A [`Secret`] has no `Display`, its `Debug` output leaves the bytes out, and its bytes are wiped
//! when it is dropped; so are the decrypted bytes [`Secret::from_decrypted`] takes.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use zeroize::Zeroize;

use crate::token::{RandomError, random_bytes};

/// Bytes in a secret: 256 bits, as long as HMAC-SHA256's output.
pub const SECRET_BYTES: usize = 32;

/// What a secret's text starts with (Standard Webhooks: a symmetric key).
pub const SECRET_PREFIX: &str = "whsec_";

/// The header that names the message (Standard Webhooks).
pub const ID_HEADER: &str = "webhook-id";

/// The header that carries the Unix time of signing, in seconds (Standard Webhooks).
pub const TIMESTAMP_HEADER: &str = "webhook-timestamp";

/// The header that carries the signature (Standard Webhooks).
pub const SIGNATURE_HEADER: &str = "webhook-signature";

/// A webhook signing secret.
pub struct Secret([u8; SECRET_BYTES]);

/// The decrypted bytes are not a secret's length: the stored value is not one this module sealed.
#[derive(Debug, thiserror::Error)]
#[error("a webhook signing secret is {SECRET_BYTES} bytes")]
pub struct MalformedSecret;

impl Secret {
    /// A new secret from the operating system's CSPRNG.
    pub fn generate() -> Result<Self, RandomError> {
        Ok(Self(random_bytes()?))
    }

    /// A secret from the bytes decryption returned, which are wiped whether or not they are a
    /// secret.
    pub fn from_decrypted(mut bytes: Vec<u8>) -> Result<Self, MalformedSecret> {
        let secret = <[u8; SECRET_BYTES]>::try_from(bytes.as_slice())
            .map(Self)
            .map_err(|_| MalformedSecret);
        bytes.zeroize();
        secret
    }

    /// The `webhook-signature` value for the message `id` sent at `timestamp` (Unix seconds)
    /// with `body`: `v1,` and HMAC-SHA256 over `id.timestamp.body` in standard padded base64.
    /// `id` must not contain `.` (Standard Webhooks), or two messages could sign alike.
    pub fn sign(&self, id: &str, timestamp: u64, body: &[u8]) -> String {
        signature(&self.0, id, timestamp, body)
    }

    /// The bytes, to encrypt for storage and to key the HMAC. Never log them.
    pub fn expose_bytes(&self) -> &[u8; SECRET_BYTES] {
        &self.0
    }

    /// `whsec_` and the bytes in standard padded base64, shown to the admin once. Never log it.
    pub fn expose_whsec(&self) -> String {
        format!("{SECRET_PREFIX}{}", STANDARD.encode(self.0))
    }
}

/// [`Secret::sign`] with any key, so the tests can check it against the Standard Webhooks
/// libraries' vectors, whose keys are 24 bytes.
fn signature(key: &[u8], id: &str, timestamp: u64, body: &[u8]) -> String {
    // Invariant: HMAC takes a key of any length, so `new_from_slice` cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_is_the_prefix_and_the_bytes_in_padded_base64() {
        let secret = Secret::generate().unwrap();
        let text = secret.expose_whsec();
        let encoded = text.strip_prefix("whsec_").expect("the prefix");
        assert_eq!(
            encoded.len(),
            44,
            "32 bytes are 44 padded base64 characters"
        );
        assert_eq!(
            STANDARD.decode(encoded).unwrap(),
            secret.expose_bytes().to_vec()
        );
    }

    #[test]
    fn each_secret_is_new() {
        let a = Secret::generate().unwrap();
        let b = Secret::generate().unwrap();
        assert_ne!(a.expose_bytes(), b.expose_bytes());
    }

    /// The vectors of the Standard Webhooks Python library's `test_sign_function` and
    /// `test_sign_function_binary_payload` (standard-webhooks/standard-webhooks,
    /// `libraries/python/tests/test_webhooks.py`): secret `whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw`.
    #[test]
    fn the_signature_matches_the_standard_webhooks_vectors() {
        let key = STANDARD.decode("MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw").unwrap();
        let (id, timestamp) = ("msg_p5jXN8AQM9LWM0D4loKWxJek", 1_614_265_330);
        assert_eq!(
            signature(&key, id, timestamp, br#"{"test": 2432232314}"#),
            "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE="
        );
        let binary: Vec<u8> = (0..=255).collect();
        assert_eq!(
            signature(&key, id, timestamp, &binary),
            "v1,sEcZ042G8Crqgxbybb3y0eDxQR3TcfYsk11I1XULhSI="
        );
    }

    #[test]
    fn a_secret_signs_with_its_bytes() {
        let secret = Secret::generate().unwrap();
        let signed = secret.sign("msg_1", 10, b"{}");
        assert_eq!(signed, signature(secret.expose_bytes(), "msg_1", 10, b"{}"));
        assert_ne!(signed, secret.sign("msg_1", 11, b"{}"));
        assert_ne!(signed, secret.sign("msg_2", 10, b"{}"));
        assert_ne!(signed, secret.sign("msg_1", 10, b"{ }"));
    }

    #[test]
    fn only_decrypted_bytes_of_a_secrets_length_are_a_secret() {
        let secret = Secret::generate().unwrap();
        let opened = Secret::from_decrypted(secret.expose_bytes().to_vec()).unwrap();
        assert_eq!(opened.expose_bytes(), secret.expose_bytes());
        assert!(Secret::from_decrypted(vec![7; SECRET_BYTES - 1]).is_err());
        assert!(Secret::from_decrypted(vec![7; SECRET_BYTES + 1]).is_err());
        assert!(Secret::from_decrypted(Vec::new()).is_err());
    }

    #[test]
    fn debug_leaves_the_secret_out() {
        let secret = Secret::generate().unwrap();
        let debug = format!("{secret:?}");
        assert_eq!(debug, "Secret(<redacted>)");
        assert!(!debug.contains(&secret.expose_whsec()[6..]));
    }
}
