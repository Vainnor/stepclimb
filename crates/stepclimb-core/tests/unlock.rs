// SPDX-License-Identifier: AGPL-3.0-or-later
//! Loading a course's published rules and checking an item's lock against a fresh database per
//! test (docs/specs/m1/04-unlock-and-completion-rules.md B6, B7, B13; T5 end to end). Needs
//! `DATABASE_URL` (`just up`).

use std::collections::HashSet;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    courses::{self, NewCourse},
    id::Uuid,
    items::{self, ItemPatch, Kind, NewItem},
    modules::{self, Error, ModulePatch, RulesPatch},
    organizations,
    rules::{CompletionRule, Requirement},
    unlock::{self, ItemRules, LockReason, RequireError, State},
};

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the credential resolvers"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An organization with one draft course in its root account.
#[derive(Clone, Copy)]
struct Org {
    id: Uuid,
    course: Uuid,
}

impl Org {
    async fn new(pool: &PgPool, slug: &str) -> Self {
        let id = organizations::create(pool, slug, slug, now())
            .await
            .expect("create organization")
            .id;
        let root: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the root");
        let new = NewCourse {
            account_id: root,
            title: "Course".into(),
            code: "C1".into(),
        };
        let course = courses::create(pool, scope(id), &new, now(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        Self { id, course }
    }

    fn scope(self) -> OrgScope {
        scope(self.id)
    }

    async fn module(self, pool: &PgPool, published: bool) -> Uuid {
        let id = modules::create(pool, self.scope(), self.course, "Module", now())
            .await
            .expect("create module")
            .id;
        let patch = ModulePatch {
            published: Some(published),
            ..ModulePatch::default()
        };
        modules::update(pool, self.scope(), id, &patch, now())
            .await
            .expect("publish module");
        id
    }

    /// An external link in `module`, last, with `requirement`.
    async fn item(
        self,
        pool: &PgPool,
        module: Uuid,
        published: bool,
        requirement: Option<Requirement>,
    ) -> Uuid {
        let new = NewItem {
            kind: Kind::ExternalLink,
            title: "Link".into(),
            ref_id: None,
            url: Some("https://example.com/".into()),
            position: None,
        };
        let id = items::create(pool, self.scope(), module, &new, now())
            .await
            .expect("create item")
            .id;
        let patch = ItemPatch {
            published: Some(published),
            ..ItemPatch::default()
        };
        items::update(pool, self.scope(), id, &patch, now())
            .await
            .expect("publish item");
        items::set_requirement(pool, self.scope(), id, requirement, now())
            .await
            .expect("set requirement");
        id
    }

    async fn rules(self, pool: &PgPool, module: Uuid, patch: RulesPatch) {
        modules::set_rules(pool, self.scope(), module, &patch, now())
            .await
            .expect("set rules");
    }
}

fn requires(ids: &[Uuid]) -> RulesPatch {
    RulesPatch {
        prerequisite_ids: Some(ids.to_vec()),
        ..RulesPatch::default()
    }
}

async fn check(
    pool: &PgPool,
    scope: OrgScope,
    item: Uuid,
    completed: &[Uuid],
) -> Result<(), RequireError> {
    let mut conn = pool.acquire().await.expect("a connection");
    let completed: HashSet<Uuid> = completed.iter().copied().collect();
    unlock::require_unlocked(&mut conn, scope, item, &completed, now()).await
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b6_load_course_rules_reads_only_published_content_in_order(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let first = a.module(&pool, true).await;
    let hidden = a.module(&pool, false).await;
    let third = a.module(&pool, true).await;
    let view = Some(Requirement::View);
    let first_items = [
        a.item(&pool, first, true, view).await,
        a.item(&pool, first, false, view).await,
        a.item(&pool, first, true, None).await,
    ];
    a.item(&pool, hidden, true, view).await;
    let at = now() + Duration::from_secs(3600);
    a.rules(
        &pool,
        third,
        RulesPatch {
            unlock_at: Some(Some(at)),
            prerequisite_ids: Some(vec![hidden, first]),
            require_sequential: Some(true),
            completion_rule: Some(CompletionRule::One),
        },
    )
    .await;

    let rules = unlock::load_course_rules(&pool, a.scope(), a.course)
        .await
        .expect("load rules");
    let ids: Vec<Uuid> = rules.modules.iter().map(|m| m.id).collect();
    assert_eq!(ids, [first, third], "the unpublished module is left out");
    assert_eq!(
        rules.modules[0].items,
        [
            ItemRules {
                id: first_items[0],
                requirement: view,
            },
            ItemRules {
                id: first_items[2],
                requirement: None,
            },
        ],
        "the unpublished item is left out"
    );
    let gated = &rules.modules[1];
    assert_eq!(gated.unlock_at, Some(at));
    assert_eq!(
        gated.prerequisite_ids,
        [first],
        "a prerequisite on an unpublished module is left out"
    );
    assert!(gated.require_sequential);
    assert_eq!(gated.completion_rule, CompletionRule::One);
    assert!(gated.items.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_unpublished_content_blocks_nothing_end_to_end(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let hidden = a.module(&pool, false).await;
    a.item(&pool, hidden, true, Some(Requirement::View)).await;
    let module = a.module(&pool, true).await;
    let unpublished = a.item(&pool, module, false, Some(Requirement::View)).await;
    let later = a.item(&pool, module, true, Some(Requirement::View)).await;
    a.rules(
        &pool,
        module,
        RulesPatch {
            require_sequential: Some(true),
            ..requires(&[hidden])
        },
    )
    .await;

    let rules = unlock::load_course_rules(&pool, a.scope(), a.course)
        .await
        .unwrap();
    let evaluation = unlock::evaluate(&rules, &HashSet::new(), now());
    assert_eq!(evaluation.module(module).unwrap().state, State::Available);
    assert_eq!(evaluation.item(later).unwrap().state, State::Available);
    assert_eq!(evaluation.item(unpublished), None);
    // Under `all`, the published requirement alone completes the module.
    let done = unlock::evaluate(&rules, &HashSet::from([later]), now());
    assert!(done.module(module).unwrap().complete);
    assert!(done.course_complete());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b13_require_unlocked_refuses_a_locked_item_with_its_reasons(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let first = a.module(&pool, true).await;
    let gate = a.item(&pool, first, true, Some(Requirement::View)).await;
    let second = a.module(&pool, true).await;
    let behind = a.item(&pool, second, true, None).await;
    a.rules(&pool, second, requires(&[first])).await;

    assert!(check(&pool, a.scope(), gate, &[]).await.is_ok());
    match check(&pool, a.scope(), behind, &[]).await {
        Err(RequireError::Locked(reasons)) => {
            assert_eq!(reasons, [LockReason::ModuleLocked]);
        }
        other => panic!("expected a lock, got {other:?}"),
    }
    assert!(check(&pool, a.scope(), behind, &[gate]).await.is_ok());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b13_require_unlocked_hides_unpublished_and_other_organizations_items(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let published = a.module(&pool, true).await;
    let hidden_module = a.module(&pool, false).await;
    let open = a.item(&pool, published, true, None).await;
    let unpublished = a.item(&pool, published, false, None).await;
    let in_hidden = a.item(&pool, hidden_module, true, None).await;

    for item in [unpublished, in_hidden, Uuid::from_u128(7)] {
        assert!(
            matches!(
                check(&pool, a.scope(), item, &[]).await,
                Err(RequireError::NotFound)
            ),
            "{item}"
        );
    }
    // Organization B's scope with A's ids.
    assert!(matches!(
        check(&pool, b.scope(), open, &[]).await,
        Err(RequireError::NotFound)
    ));
    let rules = unlock::load_course_rules(&pool, b.scope(), a.course)
        .await
        .unwrap();
    assert!(rules.modules.is_empty(), "{rules:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_stored_requirement_this_release_cannot_read_fails_closed(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool, true).await;
    let item = a.item(&pool, module, true, Some(Requirement::View)).await;
    // The column's check takes any object with a `kind`; only the typed parse refuses this one.
    sqlx::query(
        "UPDATE module_items SET requirement = '{\"kind\":\"bogus\"}'::jsonb
         WHERE org_id = $1 AND id = $2",
    )
    .bind(a.id)
    .bind(item)
    .execute(&pool)
    .await
    .expect("write a bogus requirement");

    let loaded = unlock::load_course_rules(&pool, a.scope(), a.course).await;
    assert!(
        matches!(loaded, Err(Error::Stored { id, column: "requirement" }) if id == item),
        "{loaded:?}"
    );
    let checked = check(&pool, a.scope(), item, &[]).await;
    assert!(
        matches!(
            checked,
            Err(RequireError::Repository(Error::Stored {
                column: "requirement",
                ..
            }))
        ),
        "{checked:?}"
    );
}
