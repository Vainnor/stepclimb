// SPDX-License-Identifier: AGPL-3.0-or-later
//! RSA signing keys, generated when they are needed and never written anywhere.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
#[expect(
    clippy::disallowed_types,
    reason = "the mock signs ID tokens with throwaway keys on 127.0.0.1; clippy.toml keeps RSA private keys out of every other crate (RUSTSEC-2023-0071)"
)]
use rsa::RsaPrivateKey;
use rsa::RsaPublicKey;
#[expect(
    clippy::disallowed_types,
    reason = "the mock signs ID tokens with throwaway keys on 127.0.0.1; clippy.toml keeps RSA private keys out of every other crate (RUSTSEC-2023-0071)"
)]
use rsa::pkcs1v15::SigningKey;
use rsa::rand_core::OsRng;
use rsa::sha2::Sha256;
use rsa::signature::{SignatureEncoding as _, Signer as _};
use rsa::traits::PublicKeyParts as _;
use serde_json::{Value, json};

/// The modulus size of every key: 2048 bits, the size RFC 7518 3.3 requires at least.
const BITS: usize = 2048;

/// Why a key could not be generated.
#[derive(Debug, thiserror::Error)]
#[error("could not generate an RSA key: {0}")]
pub struct KeyError(#[from] rsa::Error);

/// Why a signature could not be made.
#[derive(Debug, thiserror::Error)]
#[error("could not sign with an RSA key: {0}")]
pub struct SignError(#[from] rsa::signature::Error);

/// One RSA key pair with its `kid`, signing RS256 (RSASSA-PKCS1-v1_5 with SHA-256).
#[expect(
    clippy::disallowed_types,
    reason = "the mock signs ID tokens with throwaway keys on 127.0.0.1; clippy.toml keeps RSA private keys out of every other crate (RUSTSEC-2023-0071)"
)]
pub(crate) struct Key {
    kid: String,
    signer: SigningKey<Sha256>,
    public: RsaPublicKey,
}

impl Key {
    #[expect(
        clippy::disallowed_types,
        reason = "the mock signs ID tokens with throwaway keys on 127.0.0.1; clippy.toml keeps RSA private keys out of every other crate (RUSTSEC-2023-0071)"
    )]
    pub(crate) fn generate(kid: String) -> Result<Self, KeyError> {
        let private = RsaPrivateKey::new(&mut OsRng, BITS)?;
        let public = private.to_public_key();
        Ok(Self {
            kid,
            signer: SigningKey::new(private),
            public,
        })
    }

    pub(crate) fn kid(&self) -> &str {
        &self.kid
    }

    /// The RS256 signature of `message`.
    pub(crate) fn sign(&self, message: &[u8]) -> Result<Vec<u8>, SignError> {
        Ok(self.signer.try_sign(message)?.to_vec())
    }

    /// The public key as a JWK (RFC 7517 4, RFC 7518 6.3.1).
    pub(crate) fn jwk(&self) -> Value {
        json!({
            "kty": "RSA",
            "use": "sig",
            "alg": "RS256",
            "kid": self.kid,
            "n": URL_SAFE_NO_PAD.encode(self.public.n().to_bytes_be()),
            "e": URL_SAFE_NO_PAD.encode(self.public.e().to_bytes_be()),
        })
    }

    #[cfg(test)]
    pub(crate) fn public(&self) -> &RsaPublicKey {
        &self.public
    }
}

#[cfg(test)]
mod tests {
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::signature::Verifier as _;

    use super::*;

    #[test]
    fn a_signature_verifies_with_the_public_key_and_not_with_another() {
        let key = Key::generate("k1".to_owned()).expect("key");
        let other = Key::generate("k2".to_owned()).expect("key");
        let signature = Signature::try_from(key.sign(b"header.payload").expect("sign").as_slice())
            .expect("sig");
        VerifyingKey::<Sha256>::new(key.public().clone())
            .verify(b"header.payload", &signature)
            .expect("verifies with its own key");
        assert!(
            VerifyingKey::<Sha256>::new(other.public().clone())
                .verify(b"header.payload", &signature)
                .is_err()
        );
    }

    #[test]
    fn the_jwk_carries_the_kid_and_the_public_numbers() {
        let key = Key::generate("mock-key-1".to_owned()).expect("key");
        let jwk = key.jwk();
        assert_eq!(jwk["kid"], "mock-key-1");
        assert_eq!(jwk["kty"], "RSA");
        assert_eq!(jwk["alg"], "RS256");
        assert_eq!(jwk["e"], "AQAB");
        let n = URL_SAFE_NO_PAD
            .decode(jwk["n"].as_str().expect("n"))
            .expect("base64url");
        assert_eq!(n.len(), BITS / 8);
    }
}
