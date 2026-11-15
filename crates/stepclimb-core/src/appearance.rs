// SPDX-License-Identifier: AGPL-3.0-or-later
//! A user's appearance choice: the theme and the density the page is drawn in
//! (docs/specs/m0/11-app-shell.md B21, Q6). They are two columns of `users`; every query filters
//! by `org_id = scope.org_id()`.
//!
//! A user who has not chosen a theme has none stored (`NULL`): their pages take the
//! organization's default appearance (spec 12, the branding inputs). [`Theme::System`] is a choice
//! like the others: the page leaves `data-sc-theme` unset, so the operating system's color and
//! contrast settings select the theme. [`Density::Comfortable`] is the default density and leaves
//! `data-sc-density` unset.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
};

/// A theme a user chooses, or an organization's default appearance. The serialized names are
/// [`Theme::as_str`]'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    /// Follow the operating system.
    #[default]
    System,
    Light,
    Dark,
    HighContrast,
}

impl Theme {
    pub const ALL: [Self; 4] = [Self::System, Self::Light, Self::Dark, Self::HighContrast];

    /// The stored name, which is also the `data-sc-theme` value of every theme but `system`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::HighContrast => "high-contrast",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.as_str() == name)
    }
}

/// The density a user chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Density {
    #[default]
    Comfortable,
    /// The shorter control heights.
    Compact,
}

impl Density {
    pub const ALL: [Self; 2] = [Self::Comfortable, Self::Compact];

    /// The stored name, which is also the `data-sc-density` value of `compact`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Compact => "compact",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|density| density.as_str() == name)
    }
}

/// A user's theme and density.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Appearance {
    /// `None` until the user chooses: the organization's default appearance applies.
    pub theme: Option<Theme>,
    pub density: Density,
}

impl Appearance {
    /// The theme the page is drawn in: the user's choice, or `default` (the organization's
    /// default appearance) when they have made none.
    pub fn theme_or(self, default: Theme) -> Theme {
        self.theme.unwrap_or(default)
    }
}

/// Why an appearance call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    NotFound,
    /// The stored value is not one the column's check allows: a broken invariant.
    #[error("the stored appearance {0:?} is not a theme or density")]
    Stored(String),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

struct Row {
    theme: Option<String>,
    density: String,
}

impl TryFrom<Row> for Appearance {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let theme = match row.theme {
            None => None,
            Some(name) => Some(Theme::parse(&name).ok_or(Error::Stored(name))?),
        };
        let density = Density::parse(&row.density).ok_or(Error::Stored(row.density))?;
        Ok(Self { theme, density })
    }
}

/// The appearance of the user `user_id` of the scope's organization.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Appearance, Error> {
    sqlx::query_as!(
        Row,
        "SELECT theme, density FROM users WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        user_id,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?
    .try_into()
}

/// Set the appearance of the user `user_id` of the scope's organization, and `updated_at` to
/// `now` when it changes. The appearance the user already has writes nothing.
pub async fn set(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
    appearance: Appearance,
    now: SystemTime,
) -> Result<Appearance, Error> {
    let now = epoch_micros(now)?;
    sqlx::query_as!(
        Row,
        r#"UPDATE users
           SET theme = $3,
               density = $4,
               updated_at = CASE WHEN (theme, density) IS DISTINCT FROM ($3, $4)
                                 THEN timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
                                 ELSE updated_at END
           WHERE org_id = $1 AND id = $2
           RETURNING theme, density"#,
        scope.org_id(),
        user_id,
        appearance.theme.map(Theme::as_str),
        appearance.density.as_str(),
        now,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?
    .try_into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for theme in Theme::ALL {
            assert_eq!(Theme::parse(theme.as_str()), Some(theme));
        }
        for density in Density::ALL {
            assert_eq!(Density::parse(density.as_str()), Some(density));
        }
        assert_eq!(Theme::parse("High-Contrast"), None);
        assert_eq!(Density::parse(""), None);
    }

    #[test]
    fn serde_names_are_the_stored_names() {
        for theme in Theme::ALL {
            let json = serde_json::to_value(theme).unwrap();
            assert_eq!(json, theme.as_str());
            assert_eq!(serde_json::from_value::<Theme>(json).unwrap(), theme);
        }
    }

    #[test]
    fn a_choice_wins_over_the_default() {
        let none = Appearance::default();
        assert_eq!(none.theme_or(Theme::Dark), Theme::Dark);
        let chosen = Appearance {
            theme: Some(Theme::System),
            density: Density::Comfortable,
        };
        assert_eq!(chosen.theme_or(Theme::Dark), Theme::System);
    }
}
