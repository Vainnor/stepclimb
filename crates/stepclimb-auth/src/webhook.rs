// SPDX-License-Identifier: AGPL-3.0-or-later
//! The signing secret of a Standard Webhooks endpoint (docs/specs/m0/07-admission.md B24, Q7
//! (a)): the admission webhook's today.
//!
//! A [`Secret`] is [`SECRET_BYTES`] random bytes from the operating system's CSPRNG, inside the
//! 24 to 64 bytes the Standard Webhooks specification asks for an HMAC-SHA256 key. A receiver is
//! given it once as `whsec_` followed by the bytes in standard padded base64
//! ([`Secret::expose_whsec`]), the form the Standard Webhooks libraries read.
//!
//! A [`Secret`] has no `Display`, its `Debug` output leaves the bytes out, and its bytes are wiped
//! when it is dropped.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use zeroize::Zeroize;

use crate::token::{RandomError, random_bytes};

/// Bytes in a secret: 256 bits, as long as HMAC-SHA256's output.
pub const SECRET_BYTES: usize = 32;

/// What a secret's text starts with (Standard Webhooks: a symmetric key).
pub const SECRET_PREFIX: &str = "whsec_";

/// A webhook signing secret.
pub struct Secret([u8; SECRET_BYTES]);

impl Secret {
    /// A new secret from the operating system's CSPRNG.
    pub fn generate() -> Result<Self, RandomError> {
        Ok(Self(random_bytes()?))
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

    #[test]
    fn debug_leaves_the_secret_out() {
        let secret = Secret::generate().unwrap();
        let debug = format!("{secret:?}");
        assert_eq!(debug, "Secret(<redacted>)");
        assert!(!debug.contains(&secret.expose_whsec()[6..]));
    }
}
