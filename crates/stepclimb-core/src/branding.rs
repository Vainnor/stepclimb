// SPDX-License-Identifier: AGPL-3.0-or-later
//! The branding repository: an organization's saved branding in `organizations.branding`
//! (docs/specs/m0/12-tokens-and-theme-generator.md B21, B24, B25 and Data).
//!
//! The column holds a versioned record: the checked inputs (the generator's and the
//! organization's default appearance), the generator version, the stylesheet `/theme.css` serves,
//! its SHA-256 and the contrast reports of its themes, all written together, so what the API shows
//! always describes the stylesheet that is served. `{}`, the column's default, means the
//! organization has saved none and gets the default theme (B25). [`save`] takes only a
//! [`Rendered`], which only `theme::render` makes after every theme passed its contrast report,
//! so no stylesheet that failed the guard can be stored (B21).
//!
//! A stylesheet must declare every token the running release's bundle uses, and a newer
//! generator may add tokens. A record written by another generator version is therefore never
//! served as stored: [`effective`] re-renders it from its inputs, and [`refresh`] stores that at
//! startup for a record an older generator wrote. Inputs that fail the running generator's contrast guard get the default brand's
//! stylesheet, with the failing reports in [`Branding::fallback`] (B28).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgExecutor, Postgres};

use crate::{
    OrgScope,
    appearance::Theme,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
    theme::{self, Brand, ContrastReport, GENERATOR_VERSION, RenderError, Rendered},
};

/// The version of the stored record's format.
pub const RECORD_VERSION: u32 = 1;

/// An organization's saved branding.
#[derive(Clone, Debug, PartialEq)]
pub struct Branding {
    pub brand: Brand,
    /// The theme a user's pages take until the user chooses one.
    pub default_appearance: Theme,
    /// The generator version that wrote `css` and `report`.
    pub generator: u32,
    pub css: String,
    /// The SHA-256 of `css`, lowercase hex.
    pub css_hash: String,
    /// The light, dark and high-contrast reports of the themes in `css`.
    pub report: Vec<ContrastReport>,
    /// Set when `brand` failed this generator: `css`, `css_hash` and `report` are then the
    /// default brand's.
    pub fallback: Option<Fallback>,
}

/// Why a saved branding is served the default brand's stylesheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fallback {
    /// The reports of the saved inputs' themes under this generator, with the failing pairs;
    /// empty when the generator could not build the themes at all.
    pub report: Vec<ContrastReport>,
}

impl Branding {
    /// Whether `other` stores the same stylesheet from the same inputs. The reports are not
    /// compared: they follow from the stylesheet, and serde_json reads a stored ratio back to
    /// within a bit of the one written.
    fn same_as(&self, other: &Self) -> bool {
        let key = |b: &Self| {
            (
                b.brand.clone(),
                b.default_appearance,
                b.generator,
                b.css_hash.clone(),
                b.fallback.is_some(),
            )
        };
        key(self) == key(other)
    }

    /// The branding `rendered` stores with `default_appearance`, written by this generator
    /// version.
    pub fn from_rendered(rendered: &Rendered, default_appearance: Theme) -> Self {
        Self {
            brand: rendered.brand().clone(),
            default_appearance,
            generator: GENERATOR_VERSION,
            css: rendered.css().to_owned(),
            css_hash: css_hash(rendered.css()),
            report: rendered.reports().to_vec(),
            fallback: None,
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

/// The stored inputs: the generator's and the default appearance, which a record written before
/// it existed lacks (it then reads as `system`, what those pages drew).
#[derive(Serialize, Deserialize)]
struct Inputs {
    #[serde(flatten)]
    brand: Brand,
    #[serde(default)]
    default_appearance: Theme,
}

impl From<&Branding> for Inputs {
    fn from(b: &Branding) -> Self {
        Self {
            brand: b.brand.clone(),
            default_appearance: b.default_appearance,
        }
    }
}

/// The stored form, in `organizations.branding`.
#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    inputs: Inputs,
    generator: u32,
    css: String,
    css_hash: String,
    report: Vec<ContrastReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fallback: Option<Fallback>,
}

impl From<&Branding> for Record {
    fn from(b: &Branding) -> Self {
        Self {
            version: RECORD_VERSION,
            inputs: b.into(),
            generator: b.generator,
            css: b.css.clone(),
            css_hash: b.css_hash.clone(),
            report: b.report.clone(),
            fallback: b.fallback.clone(),
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
        brand: record.inputs.brand,
        default_appearance: record.inputs.default_appearance,
        generator: record.generator,
        css: record.css,
        css_hash: record.css_hash,
        report: record.report,
        fallback: record.fallback,
    }))
}

fn to_record_text(branding: &Branding) -> String {
    // Invariant: a struct of strings, numbers, booleans and unit enums always serializes.
    serde_json::to_string(&Record::from(branding)).expect("a branding record serializes")
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
        |b| json!({ "inputs": Inputs::from(b), "css_hash": b.css_hash }),
    )
}

/// Store `rendered` with `default_appearance` as the scope's branding, set `updated_at` to `now`,
/// and write the
/// `organization.update_branding` audit entry by `actor` with the inputs and stylesheet hash
/// before (`null` when none was saved) and after. Branding equal to the saved one writes
/// nothing, neither the row nor an entry (spec 09 B22). Returns what is saved. Runs in one
/// transaction (a savepoint when `db` is already one).
pub async fn save<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    rendered: &Rendered,
    default_appearance: Theme,
    now: SystemTime,
    actor: Actor,
) -> Result<Branding, Error> {
    let branding = Branding::from_rendered(rendered, default_appearance);
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
    let record = to_record_text(&branding);
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

/// What re-rendering saved inputs with this generator gave.
#[derive(Debug)]
pub enum Rerendered {
    /// The inputs passed: their stylesheet.
    Passed(Branding),
    /// The inputs failed: the default brand's stylesheet, `fallback` set, and why.
    FellBack {
        branding: Branding,
        error: RenderError,
    },
}

impl Rerendered {
    pub fn branding(&self) -> &Branding {
        match self {
            Self::Passed(branding) | Self::FellBack { branding, .. } => branding,
        }
    }

    pub fn into_branding(self) -> Branding {
        match self {
            Self::Passed(branding) | Self::FellBack { branding, .. } => branding,
        }
    }
}

/// `saved`'s inputs rendered by this generator. Inputs whose themes fail the contrast guard, or
/// that the generator refuses or cannot build, keep their inputs and default appearance and get
/// the default brand's stylesheet and reports, so the stylesheet still declares every token this
/// release uses (spec 12, B28).
pub fn rerender(saved: &Branding) -> Rerendered {
    outcome(saved, theme::render(&saved.brand))
}

/// [`rerender`]'s answer for the render `result` of `saved`'s inputs, apart so a test can give it
/// a contrast failure the generator never makes for an accepted input.
fn outcome(saved: &Branding, result: Result<Rendered, RenderError>) -> Rerendered {
    match result {
        Ok(rendered) => {
            Rerendered::Passed(Branding::from_rendered(&rendered, saved.default_appearance))
        }
        Err(error) => {
            let report = match &error {
                RenderError::Contrast(reports) => reports.clone(),
                RenderError::Input(_) | RenderError::NoStep(_) => Vec::new(),
            };
            // Invariant: the default brand passes every contrast pair; the build guard (spec 12
            // B22) and the server's tests check it.
            let default = theme::render(&Brand::stepclimb_default())
                .expect("the default brand renders and passes its contrast report");
            let branding = Branding {
                brand: saved.brand.clone(),
                fallback: Some(Fallback { report }),
                ..Branding::from_rendered(&default, saved.default_appearance)
            };
            Rerendered::FellBack { branding, error }
        }
    }
}

/// The branding to serve for `saved`: as stored when this generator wrote it, otherwise
/// [`rerender`]ed, so no stylesheet from another generator version is served.
pub fn effective(saved: Branding) -> Branding {
    if saved.generator == GENERATOR_VERSION {
        saved
    } else {
        rerender(&saved).into_branding()
    }
}

/// Re-render the scope's saved branding when an older generator version wrote it, and store the
/// result with its new `css_hash`, under the row's lock. Nobody changed the branding, so neither
/// `updated_at` nor the audit log is written. A record from a newer generator (a newer release
/// shares the database) is left for that release; [`effective`] re-renders it for each read
/// here. Returns what was stored, or `None` when there is no saved branding or nothing older
/// wrote it.
pub async fn refresh<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
) -> Result<Option<Rerendered>, Error> {
    let mut tx = db.begin().await?;
    let stored = sqlx::query_scalar!(
        r#"SELECT branding::text AS "branding!" FROM organizations WHERE id = $1
           FOR NO KEY UPDATE"#,
        scope.org_id(),
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let Some(saved) = parse(&stored)?.filter(|b| b.generator < GENERATOR_VERSION) else {
        return Ok(None);
    };
    let rerendered = rerender(&saved);
    sqlx::query!(
        "UPDATE organizations SET branding = $2::text::jsonb WHERE id = $1",
        scope.org_id(),
        to_record_text(rerendered.branding()),
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(rerendered))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_object_is_no_branding_and_an_unknown_version_is_an_error() {
        assert_eq!(parse("{}").unwrap(), None);
        let rendered = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        let branding = Branding::from_rendered(&rendered, Theme::Dark);
        let text = to_record_text(&branding);
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
        let branding = Branding::from_rendered(&rendered, Theme::HighContrast);
        let side = audit_side(Some(&branding));
        let keys: Vec<&String> = side.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["css_hash", "inputs"]);
        assert_eq!(side["inputs"]["default_appearance"], "high-contrast");
        assert_eq!(audit_side(None), serde_json::Value::Null);
    }

    fn saved(primary: &str, generator: u32) -> Branding {
        let brand = Brand {
            primary: primary.to_owned(),
            ..Brand::stepclimb_default()
        };
        let rendered = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        Branding {
            brand,
            generator,
            css: ":root { --sc-brand: red; }".to_owned(),
            css_hash: css_hash(":root { --sc-brand: red; }"),
            ..Branding::from_rendered(&rendered, Theme::Dark)
        }
    }

    #[test]
    fn a_record_without_a_default_appearance_reads_as_system() {
        let rendered = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        let mut record: serde_json::Value = serde_json::from_str(&to_record_text(
            &Branding::from_rendered(&rendered, Theme::Light),
        ))
        .unwrap();
        record["inputs"]
            .as_object_mut()
            .unwrap()
            .remove("default_appearance");
        let read = parse(&record.to_string()).unwrap().unwrap();
        assert_eq!(read.default_appearance, Theme::System);
        assert_eq!(read.fallback, None);
    }

    #[test]
    fn effective_serves_this_generators_record_as_stored() {
        let current = saved("#500E0E", GENERATOR_VERSION);
        assert_eq!(effective(current.clone()), current);
    }

    #[test]
    fn effective_rerenders_another_generators_record_from_its_inputs() {
        let stale = saved("#500E0E", GENERATOR_VERSION + 1);
        let served = effective(stale.clone());
        let expected = crate::theme::render(&stale.brand).unwrap();
        assert_eq!(
            served,
            Branding::from_rendered(&expected, Theme::Dark),
            "re-rendered, default appearance kept"
        );
        assert_ne!(served.css_hash, stale.css_hash);
        assert_eq!(served.generator, GENERATOR_VERSION);
        let older = saved("#500E0E", 0);
        assert_eq!(effective(older), served);
    }

    #[test]
    fn inputs_this_generator_refuses_fall_back_to_the_default_stylesheet() {
        // A chroma above the cap of the running generator, as a record from before the cap.
        let stale = saved("oklch(0.5 0.9 30)", 0);
        let Rerendered::FellBack { branding, error } = rerender(&stale) else {
            panic!("an over-cap chroma rendered");
        };
        assert!(matches!(error, RenderError::Input(_)), "{error:?}");
        let default = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        assert_eq!(branding.css, default.css());
        assert_eq!(branding.css_hash, css_hash(default.css()));
        assert_eq!(branding.brand, stale.brand, "the saved inputs are kept");
        assert_eq!(branding.default_appearance, Theme::Dark);
        assert_eq!(branding.generator, GENERATOR_VERSION);
        assert_eq!(branding.fallback, Some(Fallback { report: Vec::new() }));
    }

    #[test]
    fn a_contrast_failure_falls_back_with_the_failing_reports() {
        let stale = saved("#500E0E", 0);
        let default = crate::theme::render(&Brand::stepclimb_default()).unwrap();
        let mut failing = default.reports().to_vec();
        failing[0].pass = false;
        failing[0].pairs[9].pass = false;
        let result = outcome(&stale, Err(RenderError::Contrast(failing.clone())));
        let Rerendered::FellBack { branding, .. } = result else {
            panic!("a contrast failure was served");
        };
        assert_eq!(branding.css, default.css());
        assert_eq!(branding.report, default.reports());
        assert_eq!(branding.fallback, Some(Fallback { report: failing }));
        let text = to_record_text(&branding);
        let read = parse(&text).unwrap().unwrap();
        assert_eq!(read.fallback.map(|f| f.report.len()), Some(3));
    }
}
