// SPDX-License-Identifier: AGPL-3.0-or-later
//! Provider presets: a named set of config members a provider takes instead of writing them
//! out (docs/specs/m0/06-oauth2-and-vatsim-preset.md B18 to B20, Q4).
//!
//! This module knows no preset by name. The presets compiled into this server are data, each
//! behind its cargo feature: `vatsim` from `stepclimb-connectors` (feature `vatsim`). A server
//! built without one refuses its name with `provider.preset_unknown`.
//!
//! A preset row stores `preset` and only the members that differ from the preset's values
//! ([`Preset::strip`]): the organization's `client_id` and secret, its label, and any member it
//! overrides (Q4 (a): any member, each replacing the preset's whole, as a `PATCH` names them).
//! Every reader of a stored config reads it through [`resolve`], the preset's values under the
//! row's own, so a preset fixed in a release reaches every organization on its next read.

use std::borrow::Cow;
use std::sync::LazyLock;

use serde::Deserialize;
use serde_json::{Map, Value};
use stepclimb_auth::oauth2_config::Oauth2Config;
use stepclimb_auth::oidc_config::OidcConfig;
use stepclimb_auth::provider_config::StoredConfigError;
use stepclimb_core::identity_providers::{IdentityProvider, Kind};

/// The member of a stored config, and of the admin API's and `stepclimb.toml`'s provider, that
/// names the preset.
pub const MEMBER: &str = "preset";

/// A preset compiled into this server.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    /// The kind a provider of this preset has.
    pub kind: Kind,
    /// The key a provider takes when it names none.
    pub key: String,
    /// The label a provider takes when it names none.
    pub label: String,
    /// The config members the preset supplies.
    pub config: Map<String, Value>,
}

/// The preset a stored config or a request names is not one this server has.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("this server has no provider preset named {0:?}")]
pub struct UnknownPreset(pub String);

/// The form of a preset's data file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetFile {
    kind: String,
    key: String,
    label: String,
    config: toml::Table,
}

/// The name and TOML data of each preset compiled in.
#[cfg(feature = "vatsim")]
const SOURCES: &[(&str, &str)] = &[("vatsim", stepclimb_connectors::vatsim::PRESET)];
#[cfg(not(feature = "vatsim"))]
const SOURCES: &[(&str, &str)] = &[];

static AVAILABLE: LazyLock<Vec<Preset>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|(name, source)| {
            // The data is compiled in, and `every_compiled_preset_parses` reads each one, so a
            // failure here is a broken build, not bad input.
            parse(name, source).expect("a compiled-in preset parses")
        })
        .collect()
});

/// The presets this server has.
#[must_use]
pub fn available() -> &'static [Preset] {
    &AVAILABLE
}

/// The preset named `name`, if this server has it.
#[must_use]
pub fn find(name: &str) -> Option<&'static Preset> {
    find_in(available(), name)
}

fn find_in<'a>(presets: &'a [Preset], name: &str) -> Option<&'a Preset> {
    presets.iter().find(|preset| preset.name == name)
}

/// A preset's data file, read.
fn parse(name: &'static str, source: &str) -> Result<Preset, String> {
    let file: PresetFile = toml::from_str(source).map_err(|e| e.to_string())?;
    let kind = match Kind::parse(&file.kind) {
        Some(kind @ (Kind::Oidc | Kind::Oauth2)) => kind,
        Some(Kind::Local) | None => return Err(format!("kind {:?}", file.kind)),
    };
    let Value::Object(config) = serde_json::to_value(file.config).map_err(|e| e.to_string())?
    else {
        return Err("config is not a table".to_owned());
    };
    Ok(Preset {
        name,
        kind,
        key: file.key,
        label: file.label,
        config,
    })
}

impl Preset {
    /// `own`, a provider's members, over the preset's: each member `own` names replaces the
    /// preset's whole.
    #[must_use]
    pub fn apply(&self, own: &Map<String, Value>) -> Map<String, Value> {
        let mut config = self.config.clone();
        for (name, value) in own {
            config.insert(name.clone(), value.clone());
        }
        config
    }

    /// What a row of this preset stores for `full`, its whole validated config: the members
    /// whose value differs from the preset's, and `preset`. [`resolve`] gives `full` back.
    #[must_use]
    pub fn strip(&self, full: Map<String, Value>) -> Map<String, Value> {
        let mut stored: Map<String, Value> = full
            .into_iter()
            .filter(|(name, value)| self.config.get(name) != Some(value))
            .collect();
        stored.insert(MEMBER.into(), self.name.into());
        stored
    }
}

/// The name a stored config's `preset` member gives, if it has one.
#[must_use]
pub fn named(config: &Map<String, Value>) -> Option<&str> {
    config
        .get(MEMBER)
        .map(|value| value.as_str().unwrap_or_default())
}

/// `stored` as its readers see it: unchanged without a preset, else the preset's members under
/// the row's own, without `preset`. A preset this server does not have is [`UnknownPreset`],
/// which every reader treats as a config that does not read.
pub fn resolve(stored: &Map<String, Value>) -> Result<Cow<'_, Map<String, Value>>, UnknownPreset> {
    resolve_in(available(), stored)
}

fn resolve_in<'a>(
    presets: &[Preset],
    stored: &'a Map<String, Value>,
) -> Result<Cow<'a, Map<String, Value>>, UnknownPreset> {
    let Some(name) = named(stored) else {
        return Ok(Cow::Borrowed(stored));
    };
    let preset = find_in(presets, name).ok_or_else(|| UnknownPreset(name.to_owned()))?;
    let mut own = stored.clone();
    own.remove(MEMBER);
    Ok(Cow::Owned(preset.apply(&own)))
}

/// Why a provider's stored config does not read.
#[derive(Debug, thiserror::Error)]
pub enum StoredError {
    #[error(transparent)]
    Preset(#[from] UnknownPreset),
    #[error(transparent)]
    Config(#[from] StoredConfigError),
}

/// The `oauth2` config of `provider`'s row, resolved ([`resolve`]).
pub fn oauth2_config(provider: &IdentityProvider) -> Result<Oauth2Config, StoredError> {
    let config = resolve(&provider.config)?;
    Ok(Oauth2Config::from_stored(&config)?)
}

/// The `oidc` config of `provider`'s row, resolved ([`resolve`]).
pub fn oidc_config(provider: &IdentityProvider) -> Result<OidcConfig, StoredError> {
    let config = resolve(&provider.config)?;
    Ok(OidcConfig::from_stored(&config)?)
}

/// `provider` with its config resolved ([`resolve`]).
pub fn resolve_provider(provider: &IdentityProvider) -> Result<IdentityProvider, UnknownPreset> {
    let config = resolve(&provider.config)?.into_owned();
    Ok(IdentityProvider {
        config,
        ..provider.clone()
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        let Value::Object(map) = value else {
            panic!("an object")
        };
        map
    }

    fn sample() -> Preset {
        Preset {
            name: "sample",
            kind: Kind::Oauth2,
            key: "sample".into(),
            label: "Sign in with Sample".into(),
            config: object(json!({
                "token": "https://sample.example.test/token",
                "scopes": ["profile"],
                "claims": { "subject": "/id" },
            })),
        }
    }

    #[test]
    fn every_compiled_preset_parses() {
        for (name, source) in SOURCES {
            let preset = parse(name, source).expect("parses");
            assert_eq!(preset.name, *name);
            assert!(!preset.config.contains_key(MEMBER));
        }
        assert_eq!(available().len(), SOURCES.len());
    }

    /// Core reads these members straight from the stored row (passkey rules, email matching,
    /// the label, file ownership, admission rules), never through [`resolve`], so no preset
    /// may supply one.
    #[test]
    fn no_preset_supplies_a_member_core_reads_from_the_row() {
        let direct = [
            "allow_local_passkeys",
            "trusted_for_email_matching",
            "label",
            "source",
            "client_secret",
            "admission_rules",
        ];
        for preset in available() {
            for member in direct {
                assert!(
                    !preset.config.contains_key(member),
                    "{} {member}",
                    preset.name
                );
            }
        }
    }

    #[test]
    fn a_row_overrides_the_presets_members_whole() {
        let stored = object(json!({
            "preset": "sample",
            "client_id": "org-client",
            "claims": { "subject": "/user/id" },
        }));
        let resolved = resolve_in(&[sample()], &stored).expect("resolves");
        assert_eq!(
            Value::Object(resolved.into_owned()),
            json!({
                "token": "https://sample.example.test/token",
                "scopes": ["profile"],
                "client_id": "org-client",
                "claims": { "subject": "/user/id" },
            })
        );
    }

    #[test]
    fn strip_keeps_what_differs_and_resolve_gives_it_back() {
        let preset = sample();
        let full = object(json!({
            "token": "https://sample.example.test/token",
            "scopes": ["profile", "email"],
            "client_id": "org-client",
            "claims": { "subject": "/id" },
            "label": "Sign in",
        }));
        let stored = preset.strip(full.clone());
        assert_eq!(
            Value::Object(stored.clone()),
            json!({
                "preset": "sample",
                "scopes": ["profile", "email"],
                "client_id": "org-client",
                "label": "Sign in",
            })
        );
        let presets = [preset];
        assert_eq!(
            resolve_in(&presets, &stored)
                .expect("resolves")
                .into_owned(),
            full
        );
    }

    #[test]
    fn a_config_without_a_preset_reads_as_stored() {
        let stored = object(json!({ "client_id": "c" }));
        assert!(matches!(resolve_in(&[], &stored), Ok(Cow::Borrowed(_))));
    }

    /// A server built without a preset refuses a row naming it (spec 06 B19).
    #[test]
    fn a_preset_this_server_lacks_does_not_resolve() {
        let stored = object(json!({ "preset": "sample", "client_id": "c" }));
        assert_eq!(
            resolve_in(&[], &stored),
            Err(UnknownPreset("sample".into()))
        );
        let not_text = object(json!({ "preset": 7 }));
        assert_eq!(
            resolve_in(&[sample()], &not_text),
            Err(UnknownPreset(String::new()))
        );
    }

    /// The `vatsim` preset's anchors and attribute pointers, apart from T14: every linked
    /// identity in every organization reads them, so a change here re-points those identities
    /// or changes what admission rules read, with no admin's write. A failure means such a
    /// change was made: it needs its own review and a release note (spec 06 B20).
    #[cfg(feature = "vatsim")]
    #[test]
    fn changing_a_vatsim_anchor_or_attribute_pointer_re_points_every_linked_identity_and_needs_a_release_note()
     {
        let preset = find("vatsim").expect("compiled in");
        let origin = |member: &str| {
            let url = preset.config[member].as_str().expect("a URL");
            url::Url::parse(url)
                .expect("parses")
                .origin()
                .ascii_serialization()
        };
        for member in ["authorize", "token", "userinfo"] {
            assert_eq!(origin(member), "https://auth.vatsim.net", "{member}");
        }
        assert_eq!(
            preset.config["token"],
            "https://auth.vatsim.net/oauth/token"
        );
        assert_eq!(
            preset.config["userinfo"],
            "https://auth.vatsim.net/api/user"
        );
        let claims = &preset.config["claims"];
        assert_eq!(claims["subject"], "/data/cid");
        assert_eq!(
            claims["attributes"],
            serde_json::json!({
                "rating": "/data/vatsim/rating/short",
                "subdivision": "/data/vatsim/subdivision/id",
            })
        );
        assert_eq!(preset.kind, Kind::Oauth2);
    }
}
