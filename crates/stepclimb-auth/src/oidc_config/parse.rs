// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rules of the module docs' table, member by member, collecting every problem.

use serde_json::{Map, Value};

use super::{
    ALLOW_LOCAL_PASSKEYS, ClaimsMapping, ConfigErrors, DEFAULT_SCOPES, OidcConfig, OidcConfigDraft,
    OidcSettings, SealedSecret, StoredConfigError, TRUSTED_FOR_EMAIL_MATCHING, code,
};
use crate::json_pointer::JsonPointer;
use crate::outbound::UrlPolicy;
use crate::provider_config::walk::{self, Walk};

/// The members of an `oidc` config.
const MEMBERS: [&str; 8] = [
    "issuer",
    "client_id",
    "client_secret",
    "scopes",
    "claims",
    "userinfo",
    ALLOW_LOCAL_PASSKEYS,
    TRUSTED_FOR_EMAIL_MATCHING,
];

/// The members of `claims`.
const CLAIMS: [&str; 6] = [
    "given_name",
    "family_name",
    "email",
    "email_verified",
    "groups",
    "attributes",
];

pub(super) fn draft(
    config: &Value,
    policy: UrlPolicy,
    stored: Option<SealedSecret>,
) -> Result<OidcConfigDraft, ConfigErrors> {
    let mut walk = Walk::default();
    let Some(map) = walk::object(&mut walk, config) else {
        return walk.finish(None);
    };
    walk::unknown_members(&mut walk, &[], map, &MEMBERS);
    let settings = settings(&mut walk, map, Some(policy));
    let secret = walk::input_secret(&mut walk, map.get("client_secret"), stored);
    let draft = settings
        .zip(secret)
        .map(|(settings, secret)| OidcConfigDraft { settings, secret });
    walk.finish(draft)
}

pub(super) fn stored(config: &Map<String, Value>) -> Result<OidcConfig, StoredConfigError> {
    let mut walk = Walk::default();
    let settings = settings(&mut walk, config, None);
    let secret = walk::stored_secret(&mut walk, config);
    let config = settings
        .zip(secret)
        .map(|(settings, secret)| OidcConfig { settings, secret });
    walk.finish_stored(config)
}

/// Every member but the secret. `policy` is `None` for a stored config.
fn settings(
    walk: &mut Walk,
    map: &Map<String, Value>,
    policy: Option<UrlPolicy>,
) -> Option<OidcSettings> {
    // An issuer takes no query (OpenID Connect Discovery 1.0, section 3: no query or fragment).
    let issuer = walk::url(walk, "issuer", map.get("issuer"), policy, false);
    let client_id = walk::required_text(walk, &["client_id"], map.get("client_id"));
    let scopes = scopes(walk, map.get("scopes"));
    let claims = claims(walk, map.get("claims"));
    let userinfo = walk::flag(walk, "userinfo", map.get("userinfo"), true);
    let allow_local_passkeys = walk::flag(
        walk,
        ALLOW_LOCAL_PASSKEYS,
        map.get(ALLOW_LOCAL_PASSKEYS),
        false,
    );
    let trusted_for_email_matching = walk::flag(
        walk,
        TRUSTED_FOR_EMAIL_MATCHING,
        map.get(TRUSTED_FOR_EMAIL_MATCHING),
        false,
    );
    Some(OidcSettings {
        issuer: issuer?,
        client_id: client_id?,
        scopes: scopes?,
        claims: claims?,
        userinfo: userinfo?,
        allow_local_passkeys: allow_local_passkeys?,
        trusted_for_email_matching: trusted_for_email_matching?,
    })
}

fn scopes(walk: &mut Walk, value: Option<&Value>) -> Option<Vec<String>> {
    let scopes = walk::scopes(walk, value, &DEFAULT_SCOPES)?;
    if !scopes.iter().any(|scope| scope == "openid") {
        walk.fail(&["scopes"], code::SCOPES_MISSING_OPENID);
        return None;
    }
    Some(scopes)
}

/// A profile field's pointer, or B13's default `/<name>` when the member is absent.
fn profile_pointer(walk: &mut Walk, map: &Map<String, Value>, name: &str) -> Option<JsonPointer> {
    match walk::optional_pointer(walk, map, name)? {
        Some(pointer) => Some(pointer),
        None => JsonPointer::parse(&format!("/{name}")).ok(),
    }
}

fn claims(walk: &mut Walk, value: Option<&Value>) -> Option<ClaimsMapping> {
    let empty = Map::new();
    let map = match value {
        None => &empty,
        Some(Value::Object(map)) => map,
        Some(_) => {
            walk.fail(&["claims"], code::FIELD_TYPE);
            return None;
        }
    };
    walk::unknown_members(walk, &["claims"], map, &CLAIMS);
    let given_name = profile_pointer(walk, map, "given_name");
    let family_name = profile_pointer(walk, map, "family_name");
    let email = profile_pointer(walk, map, "email");
    let email_verified = profile_pointer(walk, map, "email_verified");
    let groups = walk::optional_pointer(walk, map, "groups");
    let attributes = walk::attributes(walk, map.get("attributes"));
    Some(ClaimsMapping {
        given_name: given_name?,
        family_name: family_name?,
        email: email?,
        email_verified: email_verified?,
        groups: groups?,
        attributes: attributes?,
    })
}
