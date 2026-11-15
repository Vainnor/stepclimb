// SPDX-License-Identifier: AGPL-3.0-or-later
//! `identity_providers::replace_config`, the writer of a provider's config that keeps its
//! admission rules (docs/specs/m0/07-admission.md I3), against a fresh database per test, and a
//! sweep that no other source file writes `identity_providers.config`. Needs `DATABASE_URL`
//! (`just up`).

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    id::{Uuid, new_id},
    identity_providers::{self, Error, IdentityProvider},
    organizations,
};

fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn later() -> SystemTime {
    now() + Duration::from_secs(60)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the routes that resolve the request's organization"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("an object, not {other}"),
    }
}

async fn provider(pool: &PgPool, org: Uuid, config: Value) -> IdentityProvider {
    sqlx::query(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled)
         VALUES ($1, $2, 'sso', 'oidc', $3, true)",
    )
    .bind(new_id())
    .bind(org)
    .bind(config)
    .execute(pool)
    .await
    .expect("insert provider");
    read(pool, org).await
}

async fn read(pool: &PgPool, org: Uuid) -> IdentityProvider {
    identity_providers::get_by_key(pool, scope(org), "sso")
        .await
        .expect("read")
        .expect("the provider exists")
}

/// The stored rules survive a config replacement; rules in the new config are dropped, so no
/// config writer can set or clear them; `updated_at` moves only when the config changes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_config_keeps_the_stored_admission_rules(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", now())
        .await
        .expect("create organization")
        .id;
    let rules = json!([{ "attribute": "subdivision", "op": "equals", "value": "USA" }]);
    let sso = provider(
        &pool,
        org,
        json!({ "client_id": "old", "admission_rules": rules }),
    )
    .await;

    let new = object(json!({ "client_id": "new", "admission_rules": [] }));
    identity_providers::replace_config(&pool, scope(org), sso.id, &new, later())
        .await
        .expect("replace");

    let stored = read(&pool, org).await;
    assert_eq!(
        Value::Object(stored.config.clone()),
        json!({ "client_id": "new", "admission_rules": rules })
    );
    assert_eq!(stored.updated_at, later());

    identity_providers::replace_config(
        &pool,
        scope(org),
        sso.id,
        &new,
        later() + Duration::from_secs(1),
    )
    .await
    .expect("replace again");
    assert_eq!(
        read(&pool, org).await.updated_at,
        later(),
        "an unchanged config"
    );
}

/// A provider without rules gets none from the new config, and another organization's
/// provider id is not found.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_config_cannot_add_rules_or_reach_another_organization(pool: PgPool) {
    let org = organizations::create(&pool, "org-a", "Org A", now())
        .await
        .expect("create organization")
        .id;
    let other = organizations::create(&pool, "org-b", "Org B", now())
        .await
        .expect("create organization")
        .id;
    let sso = provider(&pool, org, json!({ "client_id": "a" })).await;
    let smuggled = object(json!({
        "client_id": "a2",
        "admission_rules": [{ "attribute": "x", "op": "exists" }],
    }));
    identity_providers::replace_config(&pool, scope(org), sso.id, &smuggled, later())
        .await
        .expect("replace");
    assert_eq!(
        Value::Object(read(&pool, org).await.config),
        json!({ "client_id": "a2" })
    );

    let refused =
        identity_providers::replace_config(&pool, scope(other), sso.id, &smuggled, later()).await;
    assert!(matches!(refused, Err(Error::NotFound)), "{refused:?}");
}

/// Sibling sweep: `replace_config` and `admission_rules::update` are the only writers of
/// `identity_providers.config` in the workspace's sources, so no other write can drop a
/// provider's rules. A new writer must go through `replace_config`.
#[test]
fn no_other_source_writes_a_providers_config() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let allowed = [
        Path::new("stepclimb-core/src/identity_providers.rs"),
        Path::new("stepclimb-core/src/admission_rules/mod.rs"),
    ];
    let mut writers = Vec::new();
    let mut stack = vec![crates.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read a directory") {
            let path = entry.expect("an entry").path();
            let relative = path
                .strip_prefix(&crates)
                .expect("under crates")
                .to_path_buf();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "tests" && name != "target" {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs")
                && !allowed.iter().any(|a| relative == *a)
            {
                let text = std::fs::read_to_string(&path).expect("read a source file");
                let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if flat.contains("UPDATE identity_providers") && flat.contains("SET config")
                    || flat.contains("config = EXCLUDED.config")
                {
                    writers.push(relative.display().to_string());
                }
            }
        }
    }
    assert!(
        writers.is_empty(),
        "provider config written outside replace_config: {writers:?}"
    );
}
