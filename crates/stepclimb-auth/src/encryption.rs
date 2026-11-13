// SPDX-License-Identifier: AGPL-3.0-or-later
//! AES-256-GCM for secrets the server reads back (`docs/specs/m0/08-sessions-and-csrf.md` B3,
//! Q3): provider client secrets, TOTP seeds, the admission webhook secret, sign-in flow payloads.
//!
//! The keys come from configuration as a [`KeyRing`]: the first key encrypts, and every key in
//! the ring decrypts, so a key is rotated by putting a new one first and keeping the old one
//! until nothing is stored under it. The ring's text form, read from `STEPCLIMB_ENCRYPTION_KEYS`
//! or the file `STEPCLIMB_ENCRYPTION_KEYS_FILE` names, is a list of `<id>:<key>` entries
//! separated by commas or whitespace. `<id>` is 1 to 32 characters of `A-Z a-z 0-9 . _ -`, unique
//! in the ring; `<key>` is 32 bytes in standard padded base64 (`openssl rand -base64 32`).
//!
//! A sealed value is one byte string, stored in one `bytea` column:
//!
//! ```text
//! version (1 byte, 0x01) | id length (1 byte) | id | nonce (12 bytes) | ciphertext | tag (16 bytes)
//! ```
//!
//! The nonce is fresh from the operating system's CSPRNG for every value. The AEAD's associated
//! data is the version, id length and id, then the caller's context: the parts that say where the
//! value lives, for example `[b"identity_providers", org_id, row_id, b"client_secret"]`. Each
//! context part is length-prefixed (4 bytes, big-endian) before it is authenticated, so two
//! different part lists never produce the same bytes. The context is not stored; the caller
//! passes the same parts to decrypt. Changing the header, or opening the value with any other
//! context (another row, organization, table or column), fails decryption like changing the
//! ciphertext does.
//!
//! The key whose 32 bytes are [`DEVELOPMENT_KEY`] is the published local development key
//! (`deploy/compose/development-encryption-keys`). The ring accepts it, and
//! [`KeyRing::development_key_ids`] names it so the server can warn.
//!
//! Errors and `Debug` output name key ids and entry positions, never key material or plaintext.

use std::fmt;
use std::sync::Arc;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use subtle::ConstantTimeEq;

use crate::token::{RandomError, random_bytes};

/// The length of an AES-256 key.
pub const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const FORMAT_VERSION: u8 = 1;
const MAX_KEY_ID_LEN: usize = 32;

/// The bytes of the published local development key. Anything encrypted under it is readable by
/// anyone with this repository.
pub const DEVELOPMENT_KEY: [u8; KEY_BYTES] = *b"stepclimb-local-development-key!";

/// The encryption keys from configuration. Cloning is cheap; clones share the keys.
#[derive(Clone)]
pub struct KeyRing {
    /// Never empty; the first key encrypts.
    keys: Arc<[RingKey]>,
}

struct RingKey {
    id: String,
    cipher: Aes256Gcm,
    is_development_key: bool,
}

/// Why the configured key ring was refused. Entries are counted from 1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyRingError {
    #[error("holds no key; give at least one <id>:<base64 key> entry")]
    Empty,
    #[error("entry {position} is not <id>:<base64 key>")]
    NoSeparator { position: usize },
    #[error("entry {position} has a key id that is not 1 to 32 characters of A-Z a-z 0-9 . _ -")]
    BadKeyId { position: usize },
    #[error("entry {position} (id {id:?}) is not 32 bytes in standard padded base64")]
    BadKey { position: usize, id: String },
    #[error("key id {id:?} appears more than once")]
    DuplicateKeyId { id: String },
}

/// Why a value could not be encrypted.
#[derive(Debug, thiserror::Error)]
pub enum EncryptError {
    #[error("could not draw a nonce: {0}")]
    Random(#[from] RandomError),
    #[error("the value is too long for AES-GCM")]
    TooLong,
}

/// Why a sealed value could not be decrypted. None of these says which byte was wrong.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecryptError {
    #[error("the sealed value is not in a format this server knows")]
    Malformed,
    #[error("the value is sealed under key {id:?}, which is not in the key ring")]
    UnknownKey { id: String },
    #[error("the sealed value failed authentication (wrong key or altered bytes)")]
    Inauthentic,
}

impl KeyRing {
    /// Parse the ring's text form (see the module docs).
    pub fn parse(text: &str) -> Result<Self, KeyRingError> {
        let mut keys: Vec<RingKey> = Vec::new();
        let entries = text
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|entry| !entry.is_empty());
        for (index, entry) in entries.enumerate() {
            let key = parse_entry(index + 1, entry)?;
            if keys.iter().any(|existing| existing.id == key.id) {
                return Err(KeyRingError::DuplicateKeyId { id: key.id });
            }
            keys.push(key);
        }
        if keys.is_empty() {
            return Err(KeyRingError::Empty);
        }
        Ok(Self { keys: keys.into() })
    }

    /// The id of the key that encrypts new values.
    pub fn current_key_id(&self) -> &str {
        &self.current().id
    }

    /// The ids of the keys whose bytes are the published [`DEVELOPMENT_KEY`].
    pub fn development_key_ids(&self) -> Vec<&str> {
        self.keys
            .iter()
            .filter(|key| key.is_development_key)
            .map(|key| key.id.as_str())
            .collect()
    }

    /// Seal `plaintext` under the current key with a fresh nonce, bound to `context` (see the
    /// module docs).
    pub fn encrypt(&self, plaintext: &[u8], context: &[&[u8]]) -> Result<Vec<u8>, EncryptError> {
        seal(self.current(), random_bytes()?, plaintext, context)
    }

    /// Open a value sealed by [`KeyRing::encrypt`] under any key in the ring, with the same
    /// `context` it was sealed with.
    pub fn decrypt(&self, sealed: &[u8], context: &[&[u8]]) -> Result<Vec<u8>, DecryptError> {
        let (&version, rest) = sealed.split_first().ok_or(DecryptError::Malformed)?;
        let (&id_len, rest) = rest.split_first().ok_or(DecryptError::Malformed)?;
        let id_len = usize::from(id_len);
        if version != FORMAT_VERSION || rest.len() < id_len + NONCE_BYTES + TAG_BYTES {
            return Err(DecryptError::Malformed);
        }
        let (header, body) = sealed.split_at(2 + id_len);
        let (nonce, ciphertext) = body.split_at(NONCE_BYTES);
        let id = &header[2..];
        let key = self
            .keys
            .iter()
            .find(|key| key.id.as_bytes() == id)
            .ok_or_else(|| DecryptError::UnknownKey {
                id: String::from_utf8_lossy(id).into_owned(),
            })?;
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().map_err(|_| DecryptError::Malformed)?;
        key.cipher
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: &associated_data(header, context),
                },
            )
            .map_err(|_| DecryptError::Inauthentic)
    }

    fn current(&self) -> &RingKey {
        // `parse` refuses an empty ring, and no other constructor exists.
        &self.keys[0]
    }
}

impl fmt::Debug for KeyRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<&str> = self.keys.iter().map(|key| key.id.as_str()).collect();
        f.debug_struct("KeyRing").field("key_ids", &ids).finish()
    }
}

fn parse_entry(position: usize, entry: &str) -> Result<RingKey, KeyRingError> {
    let (id, encoded) = entry
        .split_once(':')
        .ok_or(KeyRingError::NoSeparator { position })?;
    let id_ok = (1..=MAX_KEY_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !id_ok {
        return Err(KeyRingError::BadKeyId { position });
    }
    // The decoder's own error names the offending byte, which is part of the key, so it is
    // dropped rather than passed on.
    let bytes: [u8; KEY_BYTES] = STANDARD
        .decode(encoded)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| KeyRingError::BadKey {
            position,
            id: id.to_owned(),
        })?;
    Ok(RingKey {
        id: id.to_owned(),
        cipher: Aes256Gcm::new(&Key::<Aes256Gcm>::from(bytes)),
        is_development_key: bytes.ct_eq(&DEVELOPMENT_KEY).into(),
    })
}

/// The header, then each context part prefixed by its length. The header carries its own id
/// length, so where it ends is never ambiguous.
fn associated_data(header: &[u8], context: &[&[u8]]) -> Vec<u8> {
    let mut data = header.to_vec();
    for part in context {
        // A context part longer than 4 GiB is a programming error: contexts are table, column
        // and id names.
        let len = u32::try_from(part.len()).expect("a context part is shorter than 4 GiB");
        data.extend_from_slice(&len.to_be_bytes());
        data.extend_from_slice(part);
    }
    data
}

fn seal(
    key: &RingKey,
    nonce: [u8; NONCE_BYTES],
    plaintext: &[u8],
    context: &[&[u8]],
) -> Result<Vec<u8>, EncryptError> {
    // `parse` holds ids to 32 bytes, so the length fits in one byte.
    let id_len = u8::try_from(key.id.len()).expect("key ids are at most 32 bytes");
    let mut sealed =
        Vec::with_capacity(2 + key.id.len() + NONCE_BYTES + plaintext.len() + TAG_BYTES);
    sealed.push(FORMAT_VERSION);
    sealed.push(id_len);
    sealed.extend_from_slice(key.id.as_bytes());
    let ciphertext = key
        .cipher
        .encrypt(
            &nonce.into(),
            Payload {
                msg: plaintext,
                aad: &associated_data(&sealed, context),
            },
        )
        .map_err(|_| EncryptError::TooLong)?;
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);
    Ok(sealed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "a:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
    const KEY_B: &str = "b:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=";
    /// Key id `a` with different key material.
    const KEY_A_OTHER: &str = "a:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=";

    /// Where a test value lives: table, organization, row, column.
    const CTX: &[&[u8]] = &[b"identity_providers", b"org-1", b"row-1", b"client_secret"];

    fn ring(text: &str) -> KeyRing {
        KeyRing::parse(text).unwrap()
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn the_cipher_matches_a_nist_vector() {
        // NIST CAVP gcmEncryptExtIV256.rsp: Keylen 256, IVlen 96, PTlen 128, AADlen 128, Tag 128.
        let bytes: [u8; 32] =
            hex("92e11dcdaa866f5ce790fd24501f92509aacf4cb8b1339d50c9c1240935dd08b")
                .try_into()
                .unwrap();
        let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(bytes));
        let nonce: [u8; 12] = hex("ac93a1a6145299bde902f21a").try_into().unwrap();
        let sealed = cipher
            .encrypt(
                &nonce.into(),
                Payload {
                    msg: &hex("2d71bcfa914e4ac045b2aa60955fad24"),
                    aad: &hex("1e0889016f67601c8ebea4943bc23ad6"),
                },
            )
            .unwrap();
        assert_eq!(
            sealed,
            hex("8995ae2e6df3dbf96fac7b7137bae67feca5aa77d51d4a0a14d9c51e1da474ab")
        );
    }

    #[test]
    fn the_sealed_layout_is_version_id_nonce_ciphertext_tag() {
        let ring = ring(KEY_A);
        let sealed = seal(ring.current(), [7; 12], b"secret", CTX).unwrap();
        assert_eq!(&sealed[..3], &[1, 1, b'a']);
        assert_eq!(&sealed[3..15], &[7; 12]);
        assert_eq!(sealed.len(), 3 + 12 + b"secret".len() + 16);
        assert_eq!(ring.decrypt(&sealed, CTX).unwrap(), b"secret");
    }

    #[test]
    fn a_value_sealed_under_a_decrypts_under_a_ring_holding_a() {
        let sealed = ring(KEY_A).encrypt(b"client secret", CTX).unwrap();
        assert_eq!(ring(KEY_A).decrypt(&sealed, CTX).unwrap(), b"client secret");
        // After rotation, b encrypts and a still decrypts.
        let rotated = ring(&format!("{KEY_B},{KEY_A}"));
        assert_eq!(rotated.current_key_id(), "b");
        assert_eq!(rotated.decrypt(&sealed, CTX).unwrap(), b"client secret");
        assert_eq!(&rotated.encrypt(b"x", CTX).unwrap()[..3], &[1, 1, b'b']);
    }

    #[test]
    fn a_ring_without_the_key_refuses_by_id() {
        let sealed = ring(KEY_A).encrypt(b"client secret", CTX).unwrap();
        assert_eq!(
            ring(KEY_B).decrypt(&sealed, CTX).unwrap_err(),
            DecryptError::UnknownKey { id: "a".to_owned() }
        );
    }

    #[test]
    fn the_same_id_with_other_key_material_fails_authentication() {
        let sealed = ring(KEY_A).encrypt(b"client secret", CTX).unwrap();
        assert_eq!(
            ring(KEY_A_OTHER).decrypt(&sealed, CTX).unwrap_err(),
            DecryptError::Inauthentic
        );
    }

    #[test]
    fn tampering_any_nonce_ciphertext_or_tag_byte_fails() {
        let ring = ring(KEY_A);
        let sealed = ring.encrypt(b"totp seed", CTX).unwrap();
        for index in 3..sealed.len() {
            let mut altered = sealed.clone();
            altered[index] ^= 0x01;
            assert_eq!(
                ring.decrypt(&altered, CTX).unwrap_err(),
                DecryptError::Inauthentic,
                "byte {index}"
            );
        }
    }

    #[test]
    fn tampering_the_header_fails() {
        let ring = ring(&format!(
            "{KEY_A},c:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
        ));
        let sealed = ring.encrypt(b"totp seed", CTX).unwrap();
        let mut version = sealed.clone();
        version[0] = 2;
        assert_eq!(
            ring.decrypt(&version, CTX).unwrap_err(),
            DecryptError::Malformed
        );
        // Pointing the value at key c, which holds the same bytes as a, still fails: the id is
        // authenticated.
        let mut id = sealed.clone();
        id[2] = b'c';
        assert_eq!(
            ring.decrypt(&id, CTX).unwrap_err(),
            DecryptError::Inauthentic
        );
    }

    #[test]
    fn short_or_empty_values_are_malformed() {
        let ring = ring(KEY_A);
        let sealed = ring.encrypt(b"", CTX).unwrap();
        assert_eq!(ring.decrypt(&sealed, CTX).unwrap(), b"");
        for length in 0..sealed.len() {
            assert_eq!(
                ring.decrypt(&sealed[..length], CTX).unwrap_err(),
                DecryptError::Malformed,
                "length {length}"
            );
        }
    }

    #[test]
    fn two_encryptions_of_one_value_differ() {
        let ring = ring(KEY_A);
        let first = ring.encrypt(b"same", CTX).unwrap();
        let second = ring.encrypt(b"same", CTX).unwrap();
        assert_ne!(first, second);
        assert_ne!(first[3..15], second[3..15], "fresh nonce");
    }

    #[test]
    fn entries_split_on_commas_and_whitespace() {
        let ring = ring(&format!("\n {KEY_B} ,\n{KEY_A}\n"));
        assert_eq!(format!("{ring:?}"), r#"KeyRing { key_ids: ["b", "a"] }"#);
    }

    #[test]
    fn bad_rings_are_refused_by_position_without_key_material() {
        let cases = [
            ("", KeyRingError::Empty),
            (" ,\n", KeyRingError::Empty),
            (
                "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
                KeyRingError::NoSeparator { position: 1 },
            ),
            (
                ":AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
                KeyRingError::BadKeyId { position: 1 },
            ),
            (
                &format!("{KEY_A},k*1:AAAA"),
                KeyRingError::BadKeyId { position: 2 },
            ),
            (
                &format!("{KEY_A},k/1:AAAA"),
                KeyRingError::BadKeyId { position: 2 },
            ),
            (
                &format!("{}:AAAA", "k".repeat(33)),
                KeyRingError::BadKeyId { position: 1 },
            ),
            (
                "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwd",
                KeyRingError::BadKey {
                    position: 1,
                    id: "k1".to_owned(),
                },
            ),
            (
                "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGx!dHh8=",
                KeyRingError::BadKey {
                    position: 1,
                    id: "k1".to_owned(),
                },
            ),
            (
                &format!("{KEY_A},{KEY_A_OTHER}"),
                KeyRingError::DuplicateKeyId { id: "a".to_owned() },
            ),
        ];
        for (text, expected) in cases {
            let error = KeyRing::parse(text).unwrap_err();
            assert_eq!(error, expected, "{text:?}");
            let message = format!("{error} {error:?}");
            assert!(
                !message.contains("AAEC") && !message.contains("dHh8"),
                "{message}"
            );
        }
    }

    #[test]
    fn a_value_moved_to_another_context_fails() {
        let ring = ring(KEY_A);
        let sealed = ring.encrypt(b"client secret", CTX).unwrap();
        let elsewhere: [&[&[u8]]; 6] = [
            &[b"identity_providers", b"org-2", b"row-1", b"client_secret"],
            &[b"identity_providers", b"org-1", b"row-2", b"client_secret"],
            &[b"admission_webhooks", b"org-1", b"row-1", b"client_secret"],
            &[b"identity_providers", b"org-1", b"row-1", b"totp_seed"],
            &[b"identity_providers", b"org-1", b"row-1"],
            &[],
        ];
        for context in elsewhere {
            assert_eq!(
                ring.decrypt(&sealed, context).unwrap_err(),
                DecryptError::Inauthentic,
                "{context:?}"
            );
        }
        assert_eq!(ring.decrypt(&sealed, CTX).unwrap(), b"client secret");
    }

    #[test]
    fn context_part_boundaries_are_authenticated() {
        let ring = ring(KEY_A);
        let sealed = ring.encrypt(b"x", &[b"ab", b"c"]).unwrap();
        assert_eq!(
            ring.decrypt(&sealed, &[b"a", b"bc"]).unwrap_err(),
            DecryptError::Inauthentic
        );
        assert_eq!(
            ring.decrypt(&sealed, &[b"abc"]).unwrap_err(),
            DecryptError::Inauthentic
        );
    }

    #[test]
    fn the_development_key_is_named_wherever_it_sits() {
        use base64::Engine as _;
        let development = STANDARD.encode(DEVELOPMENT_KEY);
        assert!(ring(KEY_A).development_key_ids().is_empty());
        let ring = ring(&format!("{KEY_A},local:{development}"));
        assert_eq!(ring.development_key_ids(), ["local"]);
    }
}
