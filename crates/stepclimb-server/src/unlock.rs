// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `item.locked` problem (docs/specs/m1/04-unlock-and-completion-rules.md B13): the answer
//! of every route that hands a learner an item's content or accepts progress for it, when
//! `stepclimb_core::unlock::require_unlocked` finds the item locked. The problem's `reasons`
//! member says what the item waits for.
//!
//! A route runs the check as
//! `unlock::require_unlocked(&mut conn, scope, item_id, &completed, state.clock.now()).await?`;
//! the conversion below turns its refusal into the answer. It runs only after the route's item
//! target has passed `require::check_course`, which hides a course the caller cannot see, a
//! draft and a template; without that check, its 403 and 404 would tell a caller whether an item
//! exists in a course they cannot see.
//!
//! [`Lock`] is the same reasons as the `lock` member of the learner reads' modules and items
//! (B12, B14), which `crate::outline` fills from the same evaluator.

use axum::http::StatusCode;
use serde::Serialize;
use stepclimb_core::{id::Uuid, unlock};
use utoipa::{
    ToSchema,
    openapi::schema::{Array, ArrayBuilder, ObjectBuilder, SchemaFormat, Type},
};

use crate::{
    error::ApiError,
    modules::content_error,
    require::{code::ITEM_NOT_FOUND, not_found_as},
    timestamp,
};

/// The codes this module produces.
pub mod code {
    /// The item is locked for the caller; `reasons` says why (403).
    pub const ITEM_LOCKED: &str = "item.locked";
}

/// One condition that keeps a module or item locked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LockReason {
    /// The module opens at `at`.
    UnlockAt {
        /// RFC 3339, UTC.
        #[schema(format = DateTime)]
        at: String,
    },
    /// The module waits for these modules of the course, in course order.
    Prerequisites {
        #[schema(schema_with = uuid_list)]
        module_ids: Vec<Uuid>,
    },
    /// The item's module is locked.
    ModuleLocked,
    /// The item waits for this earlier item of its sequential module.
    Sequential {
        #[schema(value_type = String, format = "uuid")]
        item_id: Uuid,
    },
}

/// Why a module or item is locked for you: every condition that holds, never empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct Lock {
    pub reasons: Vec<LockReason>,
}

impl Lock {
    /// The `lock` member for `state`: `None` when it is available.
    pub(crate) fn from_state(state: &unlock::State) -> Result<Option<Self>, ApiError> {
        match state {
            unlock::State::Available => Ok(None),
            unlock::State::Locked(reasons) => Ok(Some(Self {
                reasons: reasons
                    .iter()
                    .map(LockReason::from_core)
                    .collect::<Result<_, _>>()?,
            })),
        }
    }
}

/// The schema of a list of UUIDs: an array of strings in the `uuid` format.
fn uuid_list() -> Array {
    ArrayBuilder::new()
        .items(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .format(Some(SchemaFormat::Custom("uuid".to_owned()))),
        )
        .build()
}

impl LockReason {
    /// The API form of `reason`. A time `timestamp::rfc3339` cannot write, which no stored
    /// unlock date is (they are kept from 1970 up to 2200), is a server error.
    fn from_core(reason: &unlock::LockReason) -> Result<Self, ApiError> {
        Ok(match reason {
            unlock::LockReason::UnlockAt { at } => Self::UnlockAt {
                at: timestamp::rfc3339(*at).map_err(|error| ApiError::internal(&error))?,
            },
            unlock::LockReason::Prerequisites { module_ids } => Self::Prerequisites {
                module_ids: module_ids.clone(),
            },
            unlock::LockReason::ModuleLocked => Self::ModuleLocked,
            unlock::LockReason::Sequential { item_id } => Self::Sequential { item_id: *item_id },
        })
    }
}

/// 403 `item.locked` with `reasons`.
pub(crate) fn item_locked(reasons: &[unlock::LockReason]) -> ApiError {
    let reasons = match reasons.iter().map(LockReason::from_core).collect() {
        Ok(reasons) => reasons,
        Err(error) => return error,
    };
    ApiError::new(
        StatusCode::FORBIDDEN,
        code::ITEM_LOCKED,
        "The item is locked; `reasons` says what it waits for.",
    )
    .with_reasons(reasons)
}

/// 404 `item.not_found`: the item is not published in a published module, which a learner
/// cannot tell from one that does not exist.
pub(crate) fn not_found() -> ApiError {
    not_found_as(
        stepclimb_core::grants::Denied::CourseNotFound,
        ITEM_NOT_FOUND,
    )
}

impl From<unlock::RequireError> for ApiError {
    fn from(error: unlock::RequireError) -> Self {
        match error {
            unlock::RequireError::NotFound => not_found(),
            unlock::RequireError::Locked(reasons) => item_locked(&reasons),
            unlock::RequireError::Repository(error) => content_error(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use serde_json::json;

    use super::*;

    #[test]
    fn a_locked_item_answers_403_with_every_reason() {
        let (a, b, item) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let at = UNIX_EPOCH + Duration::from_secs(1_763_028_000);
        let reasons = [
            unlock::LockReason::UnlockAt { at },
            unlock::LockReason::Prerequisites {
                module_ids: vec![a, b],
            },
            unlock::LockReason::ModuleLocked,
            unlock::LockReason::Sequential { item_id: item },
        ];
        let error = ApiError::from(unlock::RequireError::Locked(reasons.to_vec()));
        assert_eq!(error.status(), StatusCode::FORBIDDEN);
        let body = serde_json::to_value(error.problem()).unwrap();
        assert_eq!(body["code"], "item.locked");
        assert_eq!(
            body["reasons"],
            json!([
                { "kind": "unlock_at", "at": "2025-11-13T10:00:00.000000Z" },
                { "kind": "prerequisites", "module_ids": [a, b] },
                { "kind": "module_locked" },
                { "kind": "sequential", "item_id": item },
            ])
        );
    }

    #[test]
    fn a_parsed_item_locked_problem_keeps_its_code_and_drops_the_reasons() {
        let error = ApiError::from(unlock::RequireError::Locked(vec![
            unlock::LockReason::ModuleLocked,
        ]));
        let body = serde_json::to_string(&error.problem()).unwrap();
        let parsed: crate::error::Problem = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.code, "item.locked");
        assert!(parsed.reasons.is_empty(), "{parsed:?}");
    }

    #[test]
    fn an_item_the_learner_cannot_see_answers_item_not_found() {
        let error = ApiError::from(unlock::RequireError::NotFound);
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
        assert_eq!(error.code(), "item.not_found");
        let body = serde_json::to_value(error.problem()).unwrap();
        assert!(body.get("reasons").is_none(), "{body}");
    }

    #[test]
    fn a_repository_failure_is_a_server_error() {
        let stored = stepclimb_core::modules::Error::Stored {
            id: Uuid::from_u128(1),
            column: "requirement",
        };
        let error = ApiError::from(unlock::RequireError::Repository(stored));
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
