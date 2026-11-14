// SPDX-License-Identifier: AGPL-3.0-or-later
//! The branding repository: an organization's saved branding in `organizations.branding`
//! (docs/specs/m0/12-tokens-and-theme-generator.md B21, B24, B25 and Data).
//!
//! The column holds a versioned record: the checked inputs, the generator version, the
//! stylesheet `/theme.css` serves and its SHA-256. `{}`, the column's default, means the
//! organization has saved none and gets the default theme (B25). [`save`] takes only a
//! [`Rendered`], which only `theme::render` makes after every theme passed its contrast report,
//! so no stylesheet that failed the guard can be stored (B21).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgExecutor, Postgres};

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
    theme::{Brand, GENERATOR_VERSION, Rendered},
};

/// The version of the stored record's format.
pub const RECORD_VERSION: u32 = 1;

/// An organization's saved branding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branding {
    pub brand: Brand,
    /// The generator version that wrote `css`.
    pub generator: u32,
    pub css: String,
    /// The SHA-256 of `css`, lowercase hex.
    pub css_hash: String,
}

impl Branding {
    /// The branding `rendered` stores, written by this generator version.
    pub fn from_rendered(rendered: &Rendered) -> Self {
        Self {
            brand: rendered.brand().clone(),
            generator: GENERATOR_VERSION,
            css: rendered.css().to_owned(),
            css_hash: css_hash(rendered.css()),
        }
    }
}

/// The SHA-256 of a stylesheet, lowercase hex.
pub fn css_hash(css: &str) -> String {
    Sha256::digest(css.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The stored form, in `organizations.branding`.
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    inputs: Brand,
    generator: u32,
    css: String,
    css_hash: String,
}

/// Why a branding repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no row.
    #[error("the organization does not exist")]
    NotFound,
    /// The stored record does not parse, or is of a version this release does not read.
    #[error("the stored branding does not parse")]
    StoredBranding(#[source] Option<serde_json::Error>),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

fn parse(stored: &str) -> Result<Option<Branding>, Error> {
    let value: serde_json::Value =
        serde_json::from_str(stored).map_err(|e| Error::StoredBranding(Some(e)))?;
    if value.as_object().is_some_and(serde_json::Map::is_empty) {
        return Ok(None);
    }
    let record: Record =
        serde_json::from_value(value).map_err(|e| Error::StoredBranding(Some(e)))?;
    if record.version != RECORD_VERSION {
        return Err(Error::StoredBranding(None));
    }
    Ok(Some(Branding {
        brand: record.inputs,
        generator: record.generator,
        css: record.css,
        css_hash: record.css_hash,
    }))
}

/// The scope's saved branding; `None` when it has saved none.
pub async fn get(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<Option<Branding>, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT branding::text AS "branding!" FROM organizations WHERE id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    parse(&stored)
}

/// Store `rendered` as the scope's branding and set `updated_at` to `now`. Branding equal to the
/// saved one writes nothing. Returns what is saved. Runs in one transaction (a savepoint when
/// `db` is already one).
pub async fn save<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    rendered: &Rendered,
    now: SystemTime,
) -> Result<Branding, Error> {
    let branding = Branding::from_rendered(rendered);
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let before = sqlx::query_scalar!(
        r#"SELECT branding::text AS "branding!" FROM organizations WHERE id = $1
           FOR NO KEY UPDATE"#,
        scope.org_id(),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    // A stored record this release cannot read is replaced, not kept.
    if parse(&before).ok().flatten().as_ref() == Some(&branding) {
        return Ok(branding);
    }
    let record = Record {
        version: RECORD_VERSION,
        inputs: branding.brand.clone(),
        generator: branding.generator,
        css: branding.css.clone(),
        css_hash: branding.css_hash.clone(),
    };
    // Invariant: a struct of strings, numbers and unit enums always serializes.
    let record = serde_json::to_string(&record).expect("a branding record serializes");
    sqlx::query!(
        "UPDATE organizations
         SET branding = $2::text::jsonb,
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE id = $1",
        scope.org_id(),
        record,
        now,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(branding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_is_no_branding_and_an_unknown_version_is_an_error() {
        assert_eq!(parse("{}").unwrap(), None);
        let rendered = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        let branding = Branding::from_rendered(&rendered);
        let record = Record {
            version: RECORD_VERSION,
            inputs: branding.brand.clone(),
            generator: branding.generator,
            css: branding.css.clone(),
            css_hash: branding.css_hash.clone(),
        };
        let text = serde_json::to_string(&record).unwrap();
        assert_eq!(parse(&text).unwrap(), Some(branding));
        let newer = text.replacen("\"version\":1", "\"version\":2", 1);
        assert!(matches!(parse(&newer), Err(Error::StoredBranding(None))));
        assert!(matches!(
            parse("{\"version\":1}"),
            Err(Error::StoredBranding(Some(_)))
        ));
    }

    #[test]
    fn the_hash_is_the_sha256_of_the_stylesheet() {
        assert_eq!(
            css_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
