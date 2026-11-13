// SPDX-License-Identifier: AGPL-3.0-or-later
//! Random tokens and their hashes (`docs/specs/m0/08-sessions-and-csrf.md` B2, B4, Q2).
//!
//! Every token is 32 bytes (256 bits) from the operating system's CSPRNG. Session tokens,
//! refresh tokens, sign-in flow tokens and authorization codes are a [`Token`]: those bytes in
//! unpadded base64url, 43 characters. API keys are an [`ApiKey`]: `sc_<random>_<checksum>`,
//! where `<random>` is the 32 bytes in base62 (43 characters, left-padded with `0`) and
//! `<checksum>` is the CRC-32 (ISO-HDLC, the zlib one) of the `sc_<random>` text in base62
//! (6 characters, left-padded), so a secret scanner or the server can reject a mistyped key
//! without a lookup.
//!
//! The server keeps only a [`TokenHash`]: SHA-256 of the token's text exactly as issued and
//! presented. A 256-bit random token needs no slow or keyed hash. `TokenHash` equality runs in
//! constant time.
//!
//! None of these types implements `Display`, and their `Debug` output never contains the secret,
//! so a token cannot reach a log line through `{}` or `{:?}`. The text leaves the type only
//! through `expose_secret`, for the cookie or the one response that hands it to its owner.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// The number of random bytes in every token and API key.
pub const TOKEN_BYTES: usize = 32;
/// The length of a [`Token`]'s text: 32 bytes in unpadded base64url.
pub const TOKEN_LEN: usize = 43;
/// The text every API key starts with.
pub const API_KEY_PREFIX: &str = "sc_";
/// The length of an [`ApiKey`]'s text: `sc_`, 43 base62 characters, `_`, 6 base62 characters.
pub const API_KEY_LEN: usize = API_KEY_PREFIX.len() + API_KEY_RANDOM_LEN + 1 + API_KEY_CHECKSUM_LEN;

/// 62^43 > 2^256, so 43 base62 digits hold any 32 bytes.
const API_KEY_RANDOM_LEN: usize = 43;
/// 62^6 > 2^32, so 6 base62 digits hold any CRC-32.
const API_KEY_CHECKSUM_LEN: usize = 6;
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const CRC32: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

/// The operating system's random number generator failed, so no token was made.
#[derive(Debug, thiserror::Error)]
#[error("the operating system's random number generator failed: {0}")]
pub struct RandomError(#[from] getrandom::Error);

/// Fill a buffer from the operating system's CSPRNG.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], RandomError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes)?;
    Ok(bytes)
}

/// A session token, refresh token, sign-in flow token or authorization code.
pub struct Token(String);

impl Token {
    /// A new token from the operating system's CSPRNG.
    pub fn generate() -> Result<Self, RandomError> {
        Ok(Self::from_random(random_bytes()?))
    }

    fn from_random(bytes: [u8; TOKEN_BYTES]) -> Self {
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// The token's text, for the cookie or response that hands it to its owner. Never log it.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// The hash the server stores and looks the token up by.
    pub fn hash(&self) -> TokenHash {
        TokenHash::of(&self.0)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// A service account's API key, `sc_<random>_<checksum>`.
pub struct ApiKey(String);

/// A presented API key is not `sc_<43 base62>_<6 base62>` with a matching checksum. The message
/// never repeats the presented text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("not a well-formed Stepclimb API key")]
pub struct ApiKeyFormatError;

impl ApiKey {
    /// A new API key from the operating system's CSPRNG.
    pub fn generate() -> Result<Self, RandomError> {
        Ok(Self::from_random(random_bytes()?))
    }

    fn from_random(bytes: [u8; TOKEN_BYTES]) -> Self {
        let body = format!("{API_KEY_PREFIX}{}", base62(&bytes, API_KEY_RANDOM_LEN));
        let checksum = base62(
            &CRC32.checksum(body.as_bytes()).to_be_bytes(),
            API_KEY_CHECKSUM_LEN,
        );
        Self(format!("{body}_{checksum}"))
    }

    /// Check a presented key's shape and checksum, without any lookup.
    pub fn parse(presented: &str) -> Result<Self, ApiKeyFormatError> {
        // ASCII first, so the byte offsets below always fall on character boundaries.
        if !presented.is_ascii()
            || presented.len() != API_KEY_LEN
            || !presented.starts_with(API_KEY_PREFIX)
        {
            return Err(ApiKeyFormatError);
        }
        let (body, rest) = presented.split_at(API_KEY_PREFIX.len() + API_KEY_RANDOM_LEN);
        let Some(checksum) = rest.strip_prefix('_') else {
            return Err(ApiKeyFormatError);
        };
        let random = &body[API_KEY_PREFIX.len()..];
        let is_base62 = |text: &str| text.bytes().all(|b| b.is_ascii_alphanumeric());
        if !is_base62(random) || !is_base62(checksum) {
            return Err(ApiKeyFormatError);
        }
        // The checksum is computed from the key's public shape, not from a secret, so an
        // ordinary comparison leaks nothing.
        let expected = base62(
            &CRC32.checksum(body.as_bytes()).to_be_bytes(),
            API_KEY_CHECKSUM_LEN,
        );
        if checksum != expected {
            return Err(ApiKeyFormatError);
        }
        Ok(Self(presented.to_owned()))
    }

    /// The key's text, for the one creation response that returns it. Never log it.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// The hash the server stores in `service_accounts.secret_hash`.
    pub fn hash(&self) -> TokenHash {
        TokenHash::of(&self.0)
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// SHA-256 of a token's text. Equality runs in constant time.
#[derive(Clone, Copy)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    /// Hash a token's text exactly as presented (a cookie value, a bearer header).
    pub fn of(presented: &str) -> Self {
        Self(Sha256::digest(presented.as_bytes()).into())
    }

    /// The 32 bytes stored in a `bytea` column.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl PartialEq for TokenHash {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for TokenHash {}

impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenHash(<redacted>)")
    }
}

/// `bytes` as a big-endian number in exactly `width` base62 digits, left-padded with `0`.
/// Every caller passes a width large enough for its byte count (see the constants above).
fn base62(bytes: &[u8], width: usize) -> String {
    let mut number = bytes.to_vec();
    let mut digits = vec![BASE62[0]; width];
    for digit in digits.iter_mut().rev() {
        let mut remainder: u32 = 0;
        for byte in &mut number {
            let value = (remainder << 8) | u32::from(*byte);
            // remainder < 62, so value < 62 * 256 and value / 62 < 256: the cast cannot truncate.
            *byte = (value / 62) as u8;
            remainder = value % 62;
        }
        *digit = BASE62[remainder as usize];
    }
    digits.into_iter().map(char::from).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// 0x00, 0x01, ..., 0x1f.
    fn counting_bytes() -> [u8; 32] {
        std::array::from_fn(|i| i as u8)
    }

    // Expected values in these tests were computed with Python's int, base64 and zlib modules,
    // not with this code.

    #[test]
    fn base62_matches_an_independent_encoding() {
        assert_eq!(base62(&u32::MAX.to_be_bytes(), 6), "4gfFC3");
        assert_eq!(
            base62(&[0xff; 32], 43),
            "yhjskwdA6OZ1AL1YmHWZWm8LLG7HjnuCA2j5rOw8Xp1"
        );
        assert_eq!(
            base62(&counting_bytes(), 43),
            "003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf"
        );
        assert_eq!(base62(&[0; 4], 6), "000000");
    }

    #[test]
    fn the_checksum_is_the_standard_crc32() {
        // The CRC-32/ISO-HDLC check value from the CRC catalog.
        assert_eq!(CRC32.checksum(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn a_token_is_unpadded_base64url_of_its_bytes() {
        let token = Token::from_random(counting_bytes());
        assert_eq!(
            token.expose_secret(),
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
        );
    }

    #[test]
    fn an_api_key_is_prefix_base62_and_crc() {
        let key = ApiKey::from_random(counting_bytes());
        assert_eq!(
            key.expose_secret(),
            "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJZ"
        );
    }

    #[test]
    fn ten_thousand_tokens_differ_and_have_the_fixed_length() {
        let tokens: HashSet<String> = (0..10_000)
            .map(|_| Token::generate().unwrap().expose_secret().to_owned())
            .collect();
        assert_eq!(tokens.len(), 10_000);
        for token in &tokens {
            assert_eq!(token.len(), TOKEN_LEN);
            assert!(
                token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            );
        }
    }

    #[test]
    fn ten_thousand_api_keys_differ_have_the_fixed_length_and_parse() {
        let keys: HashSet<String> = (0..10_000)
            .map(|_| ApiKey::generate().unwrap().expose_secret().to_owned())
            .collect();
        assert_eq!(keys.len(), 10_000);
        for key in &keys {
            assert_eq!(key.len(), API_KEY_LEN);
            assert_eq!(ApiKey::parse(key).unwrap().expose_secret(), key);
        }
    }

    #[test]
    fn a_key_with_a_wrong_checksum_is_refused() {
        let good = "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJZ";
        assert!(ApiKey::parse(good).is_ok());
        for bad in [
            "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJY",
            "sc_103aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJZ",
            "sk_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJZ",
            "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf-42WSJZ",
            "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDl!_42WSJZ",
            "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJ",
            "sc_003aUlTJC7tjlCTQj2uNU3MFagCXG9LRKRcwGkBIDlf_42WSJZZ",
            "",
        ] {
            assert_eq!(ApiKey::parse(bad).unwrap_err(), ApiKeyFormatError, "{bad}");
        }
    }

    #[test]
    fn a_non_ascii_key_of_the_right_length_is_refused_without_panicking() {
        // 53 bytes whose two-byte character spans the offset where the checksum separator sits.
        let key = format!("sc_{}é42WSJZ", "0".repeat(42));
        assert_eq!(key.len(), API_KEY_LEN);
        assert!(ApiKey::parse(&key).is_err());
    }

    #[test]
    fn the_hash_is_sha256_of_the_text() {
        let digest: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(TokenHash::of("abc").as_bytes(), &digest);
    }

    #[test]
    fn a_token_and_its_presented_text_hash_alike() {
        let token = Token::generate().unwrap();
        assert_eq!(token.hash(), TokenHash::of(token.expose_secret()));
        assert_ne!(token.hash(), Token::generate().unwrap().hash());
        let key = ApiKey::generate().unwrap();
        assert_eq!(key.hash(), TokenHash::of(key.expose_secret()));
    }

    #[test]
    fn debug_output_never_contains_the_secret() {
        let token = Token::generate().unwrap();
        let key = ApiKey::generate().unwrap();
        let printed = format!("{token:?} {key:?} {:?}", token.hash());
        assert_eq!(
            printed,
            "Token(<redacted>) ApiKey(<redacted>) TokenHash(<redacted>)"
        );
    }
}
