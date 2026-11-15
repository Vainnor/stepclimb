// SPDX-License-Identifier: AGPL-3.0-or-later
//! Local account passwords: the policy, argon2id hashes in the PHC string format, and their
//! verification (`docs/specs/m0/04-local-accounts.md` B9, B14, B16, Q8).
//!
//! [`check_policy`] is Q8 (c): 12 to 256 characters, counted as Unicode scalar values, no
//! composition rule, and not one of the 10,000 common passwords in `password/common-passwords.txt`
//! (compared in lowercase; its source is in this crate's `THIRD_PARTY.md`).
//!
//! [`hash`] computes argon2id at [`PARAMS`], Q8's OWASP Password Storage Cheat Sheet minimum
//! (19 MiB, 2 passes, 1 lane), with a 16-byte salt from the operating system's CSPRNG, and
//! returns the PHC string (`$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>`). [`verify`] checks a
//! password against a stored string with the parameters the string names, compares the outputs
//! in constant time (`password-hash`'s `Output` equality), and says whether the string should be
//! replaced because it names other parameters than [`PARAMS`] (B16's rehash). [`verify_dummy`]
//! does the same work against a fixed hash at [`PARAMS`] and always fails: the sign-in of an
//! address with no password runs it, so the response time does not say whether the account
//! exists (B14).
//!
//! These functions are CPU-bound for tens of milliseconds and allocate 19 MiB each: the server
//! runs them on blocking threads, a few at a time (B16's cap). Neither the password nor a hash
//! reaches a log line or an error message from here.

use std::{collections::HashSet, fmt, sync::OnceLock};

use argon2::{
    Algorithm, Argon2, Params, PasswordHasher as _, PasswordVerifier as _, Version,
    password_hash::{self, phc::PasswordHash},
};

/// The fewest characters a password may have (Q8).
pub const MIN_CHARS: usize = 12;
/// The most characters a password may have (Q8).
pub const MAX_CHARS: usize = 256;

/// argon2id's memory cost in KiB, the OWASP minimum (Q8): 19 MiB.
pub const MEMORY_KIB: u32 = 19 * 1024;
/// argon2id's passes over the memory, the OWASP minimum (Q8).
pub const PASSES: u32 = 2;
/// argon2id's lanes, the OWASP minimum (Q8).
pub const LANES: u32 = 1;

/// The parameters every new hash is computed with: [`MEMORY_KIB`], [`PASSES`], [`LANES`] and a
/// 32-byte output.
pub const PARAMS: Params = match Params::new(MEMORY_KIB, PASSES, LANES, Some(OUTPUT_BYTES)) {
    Ok(params) => params,
    Err(_) => panic!("Q8's argon2id parameters are within argon2's limits"),
};

const OUTPUT_BYTES: usize = 32;

/// A hash at [`PARAMS`] of a random password nobody knows, computed once with this module's
/// [`hash`]. [`verify_dummy`] verifies against it; a test checks it names [`PARAMS`].
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$aHPrLJgHBDuP+ztaZDAv8Q$YuL9PH38NqRdXtcX7i3ILTVb3Xg7yMyU7m7ngt6jBFQ";

/// The common passwords of Q8 (c), lowercase, one per line.
const COMMON: &str = include_str!("password/common-passwords.txt");

/// Why a new password is refused (422 `local.password_weak`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Weak {
    #[error("a password has at least {MIN_CHARS} characters")]
    TooShort,
    #[error("a password has at most {MAX_CHARS} characters")]
    TooLong,
    #[error("the password is one of the most common passwords")]
    Common,
}

/// A stored password hash: an argon2id PHC string. Its `Debug` output leaves the string out.
#[derive(Clone, PartialEq, Eq)]
pub struct StoredHash(String);

impl StoredHash {
    /// A hash read back from storage. [`verify`] parses it.
    pub fn new(phc: String) -> Self {
        Self(phc)
    }

    /// The PHC string, for storage.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for StoredHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StoredHash(..)")
    }
}

/// argon2 refused to compute a hash, or the CSPRNG failed to give a salt.
#[derive(Debug, thiserror::Error)]
#[error("could not hash the password: {0}")]
pub struct HashError(password_hash::Error);

/// A stored hash this module cannot verify: not a PHC string, not argon2id, or parameters
/// argon2 refuses. A server fault, since only [`hash`] writes the column.
#[derive(Debug, thiserror::Error)]
#[error("the stored password hash cannot be verified: {0}")]
pub struct VerifyError(password_hash::Error);

/// What [`verify`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verified {
    /// The password is right. `rehash` says the stored hash names other parameters than
    /// [`PARAMS`] and should be replaced by a new [`hash`] of the same password.
    Match { rehash: bool },
    /// The password is wrong.
    Mismatch,
}

/// Q8's policy for a new password (see the module documentation).
pub fn check_policy(password: &str) -> Result<(), Weak> {
    let chars = password.chars().count();
    if chars < MIN_CHARS {
        return Err(Weak::TooShort);
    }
    if chars > MAX_CHARS {
        return Err(Weak::TooLong);
    }
    if common().contains(password.to_lowercase().as_str()) {
        return Err(Weak::Common);
    }
    Ok(())
}

fn common() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| COMMON.lines().filter(|line| !line.is_empty()).collect())
}

fn argon2id() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, PARAMS)
}

/// The argon2id hash of `password` at [`PARAMS`] with a new random salt (B16).
pub fn hash(password: &str) -> Result<StoredHash, HashError> {
    let phc = argon2id()
        .hash_password(password.as_bytes())
        .map_err(HashError)?;
    Ok(StoredHash(phc.to_string()))
}

/// Check `password` against `stored` (B14, B16). One argon2id computation at the parameters
/// `stored` names.
pub fn verify(password: &str, stored: &StoredHash) -> Result<Verified, VerifyError> {
    let parsed = PasswordHash::new(&stored.0).map_err(|e| VerifyError(e.into()))?;
    if parsed.algorithm != argon2::ARGON2ID_IDENT {
        return Err(VerifyError(password_hash::Error::Algorithm));
    }
    match argon2id().verify_password(password.as_bytes(), &parsed) {
        Ok(()) => Ok(Verified::Match {
            rehash: !is_current(&parsed),
        }),
        Err(password_hash::Error::PasswordInvalid) => Ok(Verified::Mismatch),
        Err(other) => Err(VerifyError(other)),
    }
}

/// The work of [`verify`] against a hash at [`PARAMS`] no password matches, for a sign-in that
/// has no stored hash to check (B14). Always [`Verified::Mismatch`].
pub fn verify_dummy(password: &str) -> Verified {
    let dummy = StoredHash(DUMMY_HASH.to_owned());
    // The outcome is discarded: a password that happens to hash to the dummy is still no
    // account's. A test checks the dummy parses, so the computation always runs.
    let _outcome = verify(password, &dummy);
    Verified::Mismatch
}

/// Whether `parsed` was computed as [`hash`] computes today: argon2id, version 0x13, [`PARAMS`].
fn is_current(parsed: &PasswordHash) -> bool {
    let version = parsed.version == Some(Version::V0x13.into());
    let params = Params::try_from(parsed).is_ok_and(|params| params == PARAMS);
    version && params
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hash of `password` at weaker parameters than [`PARAMS`], as an older release stored.
    fn weak_hash(password: &str) -> StoredHash {
        let params = Params::new(8 * 1024, 1, 1, Some(OUTPUT_BYTES)).unwrap();
        let phc = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password(password.as_bytes())
            .unwrap();
        StoredHash(phc.to_string())
    }

    #[test]
    fn the_parameters_are_the_owasp_minimum() {
        assert_eq!(
            (PARAMS.m_cost(), PARAMS.t_cost(), PARAMS.p_cost()),
            (19_456, 2, 1)
        );
    }

    #[test]
    fn a_hash_is_argon2id_at_the_current_parameters_with_its_own_salt() {
        let first = hash("correct horse battery staple").unwrap();
        let second = hash("correct horse battery staple").unwrap();
        assert!(
            first
                .as_str()
                .starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
            "{}",
            first.as_str()
        );
        assert_ne!(first, second, "each hash has a new salt");
        let parsed = PasswordHash::new(first.as_str()).unwrap();
        assert_eq!(parsed.salt.unwrap().len(), 16);
        assert!(is_current(&parsed));
    }

    #[test]
    fn the_right_password_matches_and_a_wrong_one_does_not() {
        let stored = hash("correct horse battery staple").unwrap();
        assert_eq!(
            verify("correct horse battery staple", &stored).unwrap(),
            Verified::Match { rehash: false }
        );
        assert_eq!(
            verify("correct horse battery stapl", &stored).unwrap(),
            Verified::Mismatch
        );
        assert_eq!(
            verify("Correct horse battery staple", &stored).unwrap(),
            Verified::Mismatch
        );
    }

    #[test]
    fn a_hash_at_older_parameters_matches_and_asks_for_a_rehash() {
        let stored = weak_hash("correct horse battery staple");
        assert_eq!(
            verify("correct horse battery staple", &stored).unwrap(),
            Verified::Match { rehash: true }
        );
        assert_eq!(
            verify("wrong horse battery staple", &stored).unwrap(),
            Verified::Mismatch
        );
    }

    #[test]
    fn a_stored_string_that_is_not_argon2id_is_a_fault() {
        assert!(verify("anything at all", &StoredHash::new("plain".to_owned())).is_err());
        let argon2i = Argon2::new(Algorithm::Argon2i, Version::V0x13, PARAMS)
            .hash_password(b"correct horse battery staple")
            .unwrap()
            .to_string();
        assert!(verify("correct horse battery staple", &StoredHash::new(argon2i)).is_err());
    }

    #[test]
    fn the_dummy_hash_is_at_the_current_parameters_and_matches_nothing() {
        let parsed = PasswordHash::new(DUMMY_HASH).unwrap();
        assert_eq!(parsed.algorithm, argon2::ARGON2ID_IDENT);
        assert!(is_current(&parsed));
        assert_eq!(
            verify_dummy("correct horse battery staple"),
            Verified::Mismatch
        );
        assert_eq!(verify_dummy(""), Verified::Mismatch);
    }

    /// B14: the dummy verification is a real argon2id computation, so an address without a
    /// password costs what a wrong password does. The fastest of three runs of each, compared
    /// with a wide margin, so a busy machine does not fail it.
    #[test]
    fn the_dummy_verification_costs_what_a_real_one_does() {
        let stored = hash("correct horse battery staple").unwrap();
        let fastest = |run: &dyn Fn()| {
            (0..3)
                .map(|_| {
                    let start = std::time::Instant::now();
                    run();
                    start.elapsed()
                })
                .min()
                .unwrap()
        };
        let real = fastest(&|| {
            verify("wrong horse battery staple", &stored).unwrap();
        });
        let dummy = fastest(&|| {
            verify_dummy("wrong horse battery staple");
        });
        assert!(dummy * 4 >= real, "dummy {dummy:?}, real {real:?}");
    }

    #[test]
    fn the_policy_counts_characters_not_bytes() {
        assert_eq!(check_policy("eleven char"), Err(Weak::TooShort));
        assert_eq!(check_policy("twelve chars"), Ok(()));
        // 11 two-byte characters are 22 bytes and still too short.
        assert_eq!(check_policy(&"é".repeat(11)), Err(Weak::TooShort));
        assert_eq!(check_policy(&"é".repeat(12)), Ok(()));
        assert_eq!(check_policy(&"x".repeat(256)), Ok(()));
        assert_eq!(check_policy(&"x".repeat(257)), Err(Weak::TooLong));
    }

    #[test]
    fn a_common_password_is_refused_in_any_letter_case() {
        assert_eq!(check_policy("qwerty123456"), Err(Weak::Common));
        assert_eq!(check_policy("QWERTY123456"), Err(Weak::Common));
        assert_eq!(check_policy("1qaz2wsx3edc"), Err(Weak::Common));
        assert_eq!(check_policy("qwerty1234567 not common"), Ok(()));
    }

    #[test]
    fn the_common_list_has_ten_thousand_lowercase_entries_the_length_rule_allows() {
        let lines: Vec<&str> = COMMON.lines().collect();
        assert_eq!(lines.len(), 10_000);
        assert_eq!(common().len(), 10_000, "no entry repeats");
        for line in lines {
            let chars = line.chars().count();
            assert!((MIN_CHARS..=MAX_CHARS).contains(&chars), "{line:?}");
            assert_eq!(line, line.to_lowercase(), "{line:?}");
        }
    }

    #[test]
    fn debug_output_of_a_stored_hash_leaves_the_string_out() {
        let stored = hash("correct horse battery staple").unwrap();
        assert_eq!(format!("{stored:?}"), "StoredHash(..)");
    }
}
