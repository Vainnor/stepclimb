// SPDX-License-Identifier: AGPL-3.0-or-later
//! The branding repository: an organization's saved branding in `organizations.branding`
//! (docs/specs/m0/12-tokens-and-theme-generator.md B21, B24, B25 and Data).
//!
//! The column holds a versioned record: the checked inputs, the generator version, the
//! stylesheet `/theme.css` serves, its SHA-256 and the contrast reports of its themes, all
//! written together, so what the API shows always describes the stylesheet that is served.
//! `{}`, the column's default, means the organization has saved none and gets the default theme
//! (B25). [`save`] takes only a [`Rendered`], which only `theme::render` makes after every theme
//! passed its contrast report, so no stylesheet that failed the guard can be stored (B21).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgExecutor, Postgres};

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
    theme::{Brand, ContrastReport, GENERATOR_VERSION, Rendered},
};

/// The version of the stored record's format.
pub const RECORD_VERSION: u32 = 1;

/// An organization's saved branding.
#[derive(Clone, Debug, PartialEq)]
pub struct Branding {
    pub brand: Brand,
    /// The generator version that wrote `css` and `report`.
    pub generator: u32,
    pub css: String,
    /// The SHA-256 of `css`, lowercase hex.
    pub css_hash: String,
    /// The light, dark and high-contrast reports of the themes in `css`.
    pub report: Vec<ContrastReport>,
}

impl Branding {
    /// Whether `other` stores the same stylesheet from the same inputs. The reports are not
    /// compared: they follow from the stylesheet, and serde_json reads a stored ratio back to
    /// within a bit of the one written.
    fn same_as(&self, other: &Self) -> bool {
        (&self.brand, self.generator, &self.css_hash)
            == (&other.brand, other.generator, &other.css_hash)
    }

    /// The branding `rendered` stores, written by this generator version.
    pub fn from_rendered(rendered: &Rendered) -> Self {
        Self {
            brand: rendered.brand().clone(),
            generator: GENERATOR_VERSION,
            css: rendered.css().to_owned(),
            css_hash: css_hash(rendered.css()),
            report: rendered.reports().to_vec(),
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
    report: Vec<ContrastReport>,
}

impl From<&Branding> for Record {
    fn from(b: &Branding) -> Self {
        Self {
            version: RECORD_VERSION,
            inputs: b.brand.clone(),
            generator: b.generator,
            css: b.css.clone(),
            css_hash: b.css_hash.clone(),
            report: b.report.clone(),
        }
    }
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
        report: record.report,
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

/// The `organization.update_branding` entry's half for one side of the change: the inputs and
/// the stylesheet's hash, never the stylesheet; `null` for no saved branding.
fn audit_side(branding: Option<&Branding>) -> serde_json::Value {
    branding.map_or(
        serde_json::Value::Null,
        |b| json!({ "inputs": b.brand, "css_hash": b.css_hash }),
    )
}

/// Store `rendered` as the scope's branding, set `updated_at` to `now`, and write the
/// `organization.update_branding` audit entry by `actor` with the inputs and stylesheet hash
/// before (`null` when none was saved) and after. Branding equal to the saved one writes
/// nothing, neither the row nor an entry (spec 09 B22). Returns what is saved. Runs in one
/// transaction (a savepoint when `db` is already one).
pub async fn save<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    rendered: &Rendered,
    now: SystemTime,
    actor: Actor,
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
    let before = parse(&before).ok().flatten();
    if let Some(before) = before.as_ref().filter(|b| b.same_as(&branding)) {
        return Ok(before.clone());
    }
    // Invariant: a struct of strings, numbers, booleans and unit enums always serializes.
    let record =
        serde_json::to_string(&Record::from(&branding)).expect("a branding record serializes");
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
    let data = json!({
        "before": audit_side(before.as_ref()),
        "after": audit_side(Some(&branding)),
    });
    let update = Action::OrganizationUpdateBranding;
    audit::record(&mut tx, scope, actor, update, scope.org_id(), &data, now).await?;
    tx.commit().await?;
    // What `get` reads back from now on, so the answer of a save and a later read agree.
    parse(&record)?.ok_or(Error::StoredBranding(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_is_no_branding_and_an_unknown_version_is_an_error() {
        assert_eq!(parse("{}").unwrap(), None);
        let rendered = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        let branding = Branding::from_rendered(&rendered);
        let text = serde_json::to_string(&Record::from(&branding)).unwrap();
        let read = parse(&text).unwrap().unwrap();
        assert!(read.same_as(&branding));
        assert_eq!(read.css, branding.css);
        assert_eq!(read.report.len(), 3);
        for (r, b) in read.report.iter().zip(&branding.report) {
            assert_eq!(
                (r.appearance, r.pass, r.pairs.len()),
                (b.appearance, b.pass, b.pairs.len())
            );
        }
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

    #[test]
    fn an_audit_side_holds_the_inputs_and_hash_and_never_the_stylesheet() {
        let rendered = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        let branding = Branding::from_rendered(&rendered);
        let side = audit_side(Some(&branding));
        let keys: Vec<&String> = side.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["css_hash", "inputs"]);
        assert_eq!(audit_side(None), serde_json::Value::Null);
    }
}
