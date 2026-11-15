// SPDX-License-Identifier: AGPL-3.0-or-later
//! TOTP codes and recovery codes for local accounts (docs/specs/m0/04-local-accounts.md B19 to
//! B21, Q11).
//!
//! Q11's parameters, the ones authenticator apps accept by default: HMAC-SHA-1, 6 digits, a
//! 30-second step (RFC 6238 section 4, over RFC 4226's HOTP), and one step of drift either way
//! ([`SKEW_STEPS`]). A [`Secret`] is 20 random bytes (160 bits, RFC 4226 section 4's
//! recommendation) from the operating system's CSPRNG; the app receives it in unpadded base32
//! inside an `otpauth://` URI ([`Secret::otpauth_uri`]).
//!
//! [`verify`] accepts a code only for a step after the last one the user's code was accepted
//! for, so a code is used once (RFC 6238 section 5.2), and returns the step it matched for the
//! caller to store. The comparison runs in constant time over every step of the window.
//!
//! A [`RecoveryCode`] is 10 random bytes (80 bits) in base32, shown as four groups of four
//! characters. The server stores only its [`TokenHash`]: SHA-256 of the 16 characters without
//! the hyphens, which [`recovery_code_hash`] derives from what a user types (any letter case,
//! hyphens and spaces ignored). 80 random bits need no slow hash.
//!
//! None of the secret types implements `Display`, and their `Debug` output leaves the secret
//! out. A [`Secret`]'s bytes are wiped when it is dropped, and so are the decrypted bytes
//! [`Secret::from_decrypted`] takes. The time a code is checked at comes from the caller, which reads the controllable clock.

use std::{fmt, time::SystemTime};

use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::token::{RandomError, TokenHash, random_bytes};

/// Digits in a code (Q11).
pub const DIGITS: usize = 6;
/// Seconds in a step (Q11).
pub const PERIOD_SECS: u64 = 30;
/// Steps of drift accepted on either side of the current one (Q11).
pub const SKEW_STEPS: i64 = 1;
/// Bytes in a secret: 160 bits.
pub const SECRET_BYTES: usize = 20;
/// Recovery codes made at confirmation (Q11).
pub const RECOVERY_CODES: usize = 10;

/// Random bytes in a recovery code: 80 bits, 16 base32 characters.
const RECOVERY_BYTES: usize = 10;
/// Base32 characters in a recovery code.
const RECOVERY_LEN: usize = 16;

/// A TOTP secret.
#[derive(Clone)]
pub struct Secret([u8; SECRET_BYTES]);

/// A stored secret was not [`SECRET_BYTES`] long.
#[derive(Debug, thiserror::Error)]
#[error("a TOTP secret is {SECRET_BYTES} bytes")]
pub struct MalformedSecret;

impl Secret {
    /// A new secret from the operating system's CSPRNG.
    pub fn generate() -> Result<Self, RandomError> {
        Ok(Self(random_bytes()?))
    }

    /// A secret from its bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, MalformedSecret> {
        bytes.try_into().map(Self).map_err(|_| MalformedSecret)
    }

    /// A secret from the bytes decryption returned, which are wiped whether or not they are a
    /// secret.
    pub fn from_decrypted(mut bytes: Vec<u8>) -> Result<Self, MalformedSecret> {
        let secret = Self::from_bytes(&bytes);
        bytes.zeroize();
        secret
    }

    /// The bytes, to encrypt for storage. Never log them.
    pub fn expose_bytes(&self) -> &[u8; SECRET_BYTES] {
        &self.0
    }

    /// The secret in unpadded base32, for a user who types it into the app. Never log it.
    pub fn expose_base32(&self) -> String {
        BASE32_NOPAD.encode(&self.0)
    }

    /// The `otpauth://totp/` URI an authenticator app reads from a QR code: the label
    /// `issuer:account`, the secret, the issuer again and Q11's parameters. Every part is
    /// percent-encoded but RFC 3986's unreserved characters. It holds the secret: never log it.
    pub fn otpauth_uri(&self, issuer: &str, account: &str) -> String {
        let issuer = percent_encode(issuer);
        format!(
            "otpauth://totp/{issuer}:{account}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits={DIGITS}&period={PERIOD_SECS}",
            account = percent_encode(account),
            secret = self.expose_base32(),
        )
    }

    /// The secret an app was given as `text`, unpadded base32, for tests that play the app.
    /// Only a `test-support` build has it.
    #[cfg(feature = "test-support")]
    pub fn from_base32(text: &str) -> Option<Self> {
        let bytes = BASE32_NOPAD.decode(text.as_bytes()).ok()?;
        Self::from_bytes(&bytes).ok()
    }

    /// The code an authenticator app shows for this secret at `now`, for tests that play the
    /// app. Only a `test-support` build has it: the server never computes a code to hand out.
    #[cfg(feature = "test-support")]
    pub fn code_at_time(&self, now: SystemTime) -> String {
        let step = u64::try_from(step_at(now)).unwrap_or_default();
        String::from_utf8_lossy(&self.code_at(step)).into_owned()
    }

    /// The code for `step` (RFC 4226 section 5.3's dynamic truncation of HMAC-SHA-1 over the
    /// step as an 8-byte big-endian counter), as its [`DIGITS`] ASCII digits.
    fn code_at(&self, step: u64) -> [u8; DIGITS] {
        // Invariant: HMAC takes a key of any length, so `new_from_slice` cannot fail.
        let mut mac = Hmac::<Sha1>::new_from_slice(&self.0).expect("HMAC takes any key length");
        mac.update(&step.to_be_bytes());
        let digest = mac.finalize().into_bytes();
        let offset = usize::from(digest[digest.len() - 1] & 0x0f);
        let binary = u32::from_be_bytes([
            digest[offset] & 0x7f,
            digest[offset + 1],
            digest[offset + 2],
            digest[offset + 3],
        ]);
        let mut value = binary % 10u32.pow(DIGITS as u32);
        let mut code = [b'0'; DIGITS];
        for digit in code.iter_mut().rev() {
            // value % 10 < 10, so the cast cannot truncate.
            *digit = b'0' + (value % 10) as u8;
            value /= 10;
        }
        code
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

/// A presented TOTP code: [`DIGITS`] ASCII digits.
#[derive(Clone, Copy)]
pub struct Code([u8; DIGITS]);

impl Code {
    /// `presented` as a code, with spaces ignored (apps show `123 456`), or `None` when it is
    /// not [`DIGITS`] ASCII digits.
    pub fn parse(presented: &str) -> Option<Self> {
        let mut code = [0u8; DIGITS];
        let mut digits = presented.bytes().filter(|byte| *byte != b' ');
        for slot in &mut code {
            *slot = digits.next().filter(u8::is_ascii_digit)?;
        }
        digits.next().is_none().then_some(Self(code))
    }
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Code(<redacted>)")
    }
}

/// The step `now` falls in: seconds since the Unix epoch divided by [`PERIOD_SECS`]. A time
/// before the epoch is step 0.
pub fn step_at(now: SystemTime) -> i64 {
    let secs = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    i64::try_from(secs / PERIOD_SECS).unwrap_or(i64::MAX)
}

/// The step `code` matches for `secret` at `now`, within [`SKEW_STEPS`] of the current step and
/// after `last_step` (the step of the last code accepted for the user), or `None`. Every step
/// of the window is compared, in constant time, whatever matches; when two would match, the
/// later is returned, so neither can be used again.
pub fn verify(
    secret: &Secret,
    code: &Code,
    now: SystemTime,
    last_step: Option<i64>,
) -> Option<i64> {
    let current = step_at(now);
    let mut matched = None;
    for step in current.saturating_sub(SKEW_STEPS)..=current.saturating_add(SKEW_STEPS) {
        let Ok(counter) = u64::try_from(step) else {
            continue;
        };
        let equal: bool = secret.code_at(counter).ct_eq(&code.0).into();
        let fresh = last_step.is_none_or(|last| step > last);
        if equal && fresh {
            matched = Some(step);
        }
    }
    matched
}

/// A recovery code, shown to the user once.
pub struct RecoveryCode(String);

impl RecoveryCode {
    /// The code as the user sees it: `ABCD-EFGH-IJKL-MNOP`. Never log it.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }

    /// The hash the server stores.
    pub fn hash(&self) -> TokenHash {
        TokenHash::of(&self.0.replace('-', ""))
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode(<redacted>)")
    }
}

/// [`RECOVERY_CODES`] new recovery codes from the operating system's CSPRNG.
pub fn generate_recovery_codes() -> Result<Vec<RecoveryCode>, RandomError> {
    (0..RECOVERY_CODES)
        .map(|_| {
            let text = BASE32_NOPAD.encode(&random_bytes::<RECOVERY_BYTES>()?);
            let groups: Vec<&str> = (0..RECOVERY_LEN)
                .step_by(4)
                .map(|start| &text[start..start + 4])
                .collect();
            Ok(RecoveryCode(groups.join("-")))
        })
        .collect()
}

/// The stored hash of the recovery code `presented` names: hyphens and spaces dropped, letters
/// uppercased. `None` when what is left is not 16 base32 characters, so it matches no code.
pub fn recovery_code_hash(presented: &str) -> Option<TokenHash> {
    let normalized: String = presented
        .chars()
        .filter(|c| *c != '-' && *c != ' ')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let base32 = |c: char| c.is_ascii_uppercase() || ('2'..='7').contains(&c);
    (normalized.len() == RECOVERY_LEN && normalized.chars().all(base32))
        .then(|| TokenHash::of(&normalized))
}

/// `text` with every byte but RFC 3986's unreserved characters percent-encoded.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// RFC 4226 Appendix D's and RFC 6238 Appendix B's SHA-1 secret, "12345678901234567890".
    fn rfc_secret() -> Secret {
        Secret::from_bytes(b"12345678901234567890").unwrap()
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn code(text: &str) -> Code {
        Code::parse(text).unwrap()
    }

    #[test]
    fn hotp_matches_rfc_4226_appendix_d() {
        let expected = [
            "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583",
            "399871", "520489",
        ];
        for (counter, want) in expected.iter().enumerate() {
            let got = rfc_secret().code_at(counter as u64);
            assert_eq!(
                std::str::from_utf8(&got).unwrap(),
                *want,
                "counter {counter}"
            );
        }
    }

    /// RFC 6238 Appendix B's SHA-1 rows give 8 digits; 6 digits are the last 6, since both
    /// are the same truncated value modulo a power of ten.
    #[test]
    fn totp_matches_rfc_6238_appendix_b() {
        let rows = [
            (59, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ];
        for (secs, eight) in rows {
            let step = step_at(at(secs));
            let got = rfc_secret().code_at(step as u64);
            assert_eq!(std::str::from_utf8(&got).unwrap(), &eight[2..], "{secs}");
        }
    }

    #[test]
    fn a_code_is_accepted_one_step_either_way_and_no_further() {
        let secret = rfc_secret();
        let now = at(1_111_111_111);
        let step = step_at(now);
        let code_for = |offset: i64| {
            let bytes = secret.code_at((step + offset) as u64);
            code(std::str::from_utf8(&bytes).unwrap())
        };
        for offset in [-1, 0, 1] {
            assert_eq!(
                verify(&secret, &code_for(offset), now, None),
                Some(step + offset)
            );
        }
        for offset in [-2, 2] {
            assert_eq!(verify(&secret, &code_for(offset), now, None), None);
        }
    }

    #[test]
    fn a_code_at_or_before_the_last_accepted_step_is_refused() {
        let secret = rfc_secret();
        let now = at(1_111_111_111);
        let step = step_at(now);
        let current = code(std::str::from_utf8(&secret.code_at(step as u64)).unwrap());
        assert_eq!(verify(&secret, &current, now, Some(step - 1)), Some(step));
        assert_eq!(verify(&secret, &current, now, Some(step)), None);
        assert_eq!(verify(&secret, &current, now, Some(step + 1)), None);
    }

    #[test]
    fn a_code_is_six_digits_with_spaces_ignored() {
        assert!(Code::parse("123456").is_some());
        assert!(Code::parse("123 456").is_some());
        for bad in ["12345", "1234567", "12345a", "", "١٢٣٤٥٦", "123-456"] {
            assert!(Code::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_uri_carries_the_secret_and_q11s_parameters() {
        let secret = rfc_secret();
        assert_eq!(secret.expose_base32(), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(
            secret.otpauth_uri("Org A & Co", "sam@example.test"),
            "otpauth://totp/Org%20A%20%26%20Co:sam%40example.test?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Org%20A%20%26%20Co&algorithm=SHA1&digits=6&period=30"
        );
        let colon = secret.otpauth_uri("a:b", "c");
        assert!(colon.starts_with("otpauth://totp/a%3Ab:c?"), "{colon}");
    }

    #[test]
    fn generated_secrets_differ_and_round_trip() {
        let one = Secret::generate().unwrap();
        let two = Secret::generate().unwrap();
        assert_ne!(one.expose_bytes(), two.expose_bytes());
        let back = Secret::from_bytes(one.expose_bytes()).unwrap();
        assert_eq!(back.expose_bytes(), one.expose_bytes());
        assert!(Secret::from_bytes(&[0; 19]).is_err());
        let decrypted = Secret::from_decrypted(one.expose_bytes().to_vec()).unwrap();
        assert_eq!(decrypted.expose_bytes(), one.expose_bytes());
        assert!(Secret::from_decrypted(vec![0; 21]).is_err());
    }

    #[test]
    fn recovery_codes_are_ten_distinct_and_match_their_hash_however_typed() {
        let codes = generate_recovery_codes().unwrap();
        assert_eq!(codes.len(), RECOVERY_CODES);
        let mut texts: Vec<&str> = codes.iter().map(RecoveryCode::expose_secret).collect();
        texts.sort_unstable();
        texts.dedup();
        assert_eq!(texts.len(), RECOVERY_CODES);
        let first = &codes[0];
        let shown = first.expose_secret();
        assert_eq!(shown.len(), 19);
        assert_eq!(shown.matches('-').count(), 3);
        let typed = shown.to_lowercase().replace('-', " ");
        assert_eq!(recovery_code_hash(shown), Some(first.hash()));
        assert_eq!(recovery_code_hash(&typed), Some(first.hash()));
        for bad in ["123456", "", "ABCD-EFGH-IJKL-MNO", "ABCD-EFGH-IJKL-MN01"] {
            assert!(recovery_code_hash(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn debug_output_never_contains_a_secret() {
        let secret = rfc_secret();
        let recovery = generate_recovery_codes().unwrap().remove(0);
        let printed = format!("{secret:?} {:?} {recovery:?}", code("123456"));
        assert!(!printed.contains("GEZDG"), "{printed}");
        assert!(!printed.contains("123456"), "{printed}");
        assert!(!printed.contains(recovery.expose_secret()), "{printed}");
    }
}
