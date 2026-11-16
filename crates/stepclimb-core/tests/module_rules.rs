// SPDX-License-Identifier: AGPL-3.0-or-later
//! Module rules and item requirements against a fresh database per test
//! (docs/specs/m1/04-unlock-and-completion-rules.md B1 to B3, Data; T9 and T10 at the
//! repository level). Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    courses::{self, NewCourse, Status},
    id::{Uuid, new_id},
    items::{self, Kind, NewItem},
    modules::{self, Error, RulesPatch},
    organizations,
    rules::{CompletionRule, Requirement},
};

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn later() -> SystemTime {
    now() + Duration::from_secs(60)
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
        let mut org = Self {
            id,
            course: Uuid::nil(),
        };
        org.course = org.course(pool, "C1").await;
        org
    }

    fn scope(self) -> OrgScope {
        scope(self.id)
    }

    async fn course(self, pool: &PgPool, code: &str) -> Uuid {
        let root: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(self.id)
            .fetch_one(pool)
            .await
            .expect("the root");
        let new = NewCourse {
            account_id: root,
            title: format!("Course {code}"),
            code: code.into(),
        };
        courses::create(pool, self.scope(), &new, now(), Actor::Instance)
            .await
            .expect("create course")
            .id
    }

    async fn module(self, pool: &PgPool) -> Uuid {
        self.module_in(pool, self.course).await
    }

    async fn module_in(self, pool: &PgPool, course: Uuid) -> Uuid {
        modules::create(pool, self.scope(), course, "Module", now())
            .await
            .expect("create module")
            .id
    }

    async fn item(self, pool: &PgPool, module: Uuid, kind: Kind) -> Uuid {
        let new = NewItem {
            kind,
            title: "Item".into(),
            ref_id: None,
            url: (kind == Kind::ExternalLink).then(|| "https://example.com/".into()),
            position: None,
        };
        items::create(pool, self.scope(), module, &new, now())
            .await
            .expect("create item")
            .id
    }

    /// An item of a kind no route creates yet (page, file, video), inserted directly, last in
    /// `module`, with a stand-in `ref_id`.
    async fn raw_item(self, pool: &PgPool, module: Uuid, kind: &str) -> Uuid {
        let id = new_id();
        sqlx::query(
            "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, ref_id,
                                       title)
             SELECT $1, $2, $3, $4, count(*), $5, $6, 'Raw'
             FROM module_items WHERE module_id = $4",
        )
        .bind(id)
        .bind(self.id)
        .bind(self.course)
        .bind(module)
        .bind(kind)
        .bind(new_id())
        .execute(pool)
        .await
        .expect("insert item");
        id
    }
}

fn requires(ids: &[Uuid]) -> RulesPatch {
    RulesPatch {
        prerequisite_ids: Some(ids.to_vec()),
        ..RulesPatch::default()
    }
}

async fn set(pool: &PgPool, org: Org, module: Uuid, patch: &RulesPatch) -> Result<(), Error> {
    modules::set_rules(pool, org.scope(), module, patch, later())
        .await
        .map(|_| ())
}

/// Every prerequisite row of `org`, as text.
async fn prerequisite_rows(pool: &PgPool, org: Org) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT to_jsonb(p)::text FROM module_prerequisites p WHERE org_id = $1 ORDER BY 1",
    )
    .bind(org.id)
    .fetch_all(pool)
    .await
    .expect("read prerequisites")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b1_rules_are_stored_and_a_same_value_patch_writes_nothing(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool).await;
    let fresh = modules::get(&pool, a.scope(), module)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fresh.unlock_at, None);
    assert!(fresh.prerequisite_ids.is_empty());
    assert!(!fresh.require_sequential);
    assert_eq!(fresh.completion_rule, CompletionRule::All);

    let at = UNIX_EPOCH + Duration::from_secs(1_900_000_000);
    let patch = RulesPatch {
        unlock_at: Some(Some(at)),
        prerequisite_ids: None,
        require_sequential: Some(true),
        completion_rule: Some(CompletionRule::One),
    };
    let changed = modules::set_rules(&pool, a.scope(), module, &patch, later())
        .await
        .unwrap();
    assert_eq!(changed.unlock_at, Some(at));
    assert!(changed.require_sequential);
    assert_eq!(changed.completion_rule, CompletionRule::One);
    assert_eq!(changed.updated_at, later());

    let again = modules::set_rules(
        &pool,
        a.scope(),
        module,
        &patch,
        later() + Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(again.updated_at, later(), "nothing written");

    let cleared = RulesPatch {
        unlock_at: Some(None),
        ..RulesPatch::default()
    };
    let cleared = modules::set_rules(&pool, a.scope(), module, &cleared, later())
        .await
        .unwrap();
    assert_eq!(cleared.unlock_at, None);
    for bad in [
        UNIX_EPOCH - Duration::from_secs(1),
        UNIX_EPOCH + Duration::from_secs(7_258_118_400),
    ] {
        let patch = RulesPatch {
            unlock_at: Some(Some(bad)),
            ..RulesPatch::default()
        };
        let refused = set(&pool, a, module, &patch).await;
        assert!(
            matches!(refused, Err(Error::UnlockAtOutOfRange)),
            "{refused:?}"
        );
    }
}

/// T9.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_prerequisite_rules(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let (ma, mb, mc) = (
        a.module(&pool).await,
        a.module(&pool).await,
        a.module(&pool).await,
    );

    // Stored in course order, whatever order the request names them in.
    set(&pool, a, mc, &requires(&[mb, ma])).await.unwrap();
    let c = modules::get(&pool, a.scope(), mc).await.unwrap().unwrap();
    assert_eq!(c.prerequisite_ids, [ma, mb]);

    // A -> B -> A, and a longer cycle through C.
    set(&pool, a, mb, &requires(&[ma])).await.unwrap();
    let before = prerequisite_rows(&pool, a).await;
    let refused = set(&pool, a, ma, &requires(&[mb])).await;
    assert!(
        matches!(refused, Err(Error::PrerequisiteCycle)),
        "{refused:?}"
    );
    let refused = set(&pool, a, ma, &requires(&[mc])).await;
    assert!(
        matches!(refused, Err(Error::PrerequisiteCycle)),
        "{refused:?}"
    );
    assert_eq!(prerequisite_rows(&pool, a).await, before, "nothing changed");

    let refused = set(&pool, a, ma, &requires(&[ma])).await;
    assert!(
        matches!(refused, Err(Error::PrerequisiteSelf)),
        "{refused:?}"
    );
    let refused = set(&pool, a, ma, &requires(&[mb, mb])).await;
    assert!(
        matches!(refused, Err(Error::PrerequisiteDuplicate)),
        "{refused:?}"
    );
    let other_course = a.course(&pool, "C2").await;
    let elsewhere = a.module_in(&pool, other_course).await;
    let theirs = b.module(&pool).await;
    for foreign in [elsewhere, theirs, new_id()] {
        let refused = set(&pool, a, mc, &requires(&[foreign])).await;
        assert!(
            matches!(refused, Err(Error::PrerequisiteNotFound)),
            "{refused:?}"
        );
    }
    let many: Vec<Uuid> = (0..21).map(|_| new_id()).collect();
    let refused = set(&pool, a, ma, &requires(&many)).await;
    assert!(
        matches!(refused, Err(Error::PrerequisitesTooMany)),
        "{refused:?}"
    );
    let mut twenty = Vec::new();
    for _ in 0..20 {
        twenty.push(a.module(&pool).await);
    }
    set(&pool, a, ma, &requires(&twenty)).await.unwrap();
    assert_eq!(prerequisite_rows(&pool, a).await.len(), 2 + 1 + 20);

    // The whole set is replaced; an empty set clears it.
    set(&pool, a, mc, &requires(&[mb])).await.unwrap();
    let c = modules::get(&pool, a.scope(), mc).await.unwrap().unwrap();
    assert_eq!(c.prerequisite_ids, [mb]);
    set(&pool, a, mc, &requires(&[])).await.unwrap();
    let c = modules::get(&pool, a.scope(), mc).await.unwrap().unwrap();
    assert!(c.prerequisite_ids.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b2_deleting_a_module_removes_the_prerequisites_that_name_it(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (ma, mb) = (a.module(&pool).await, a.module(&pool).await);
    set(&pool, a, mb, &requires(&[ma])).await.unwrap();
    modules::delete(&pool, a.scope(), ma).await.unwrap();
    let b = modules::get(&pool, a.scope(), mb).await.unwrap().unwrap();
    assert!(b.prerequisite_ids.is_empty());
    assert!(prerequisite_rows(&pool, a).await.is_empty());
}

/// T9: two writes that each close half of A -> B -> A; the second waits on the course lock,
/// sees the first, and is refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_concurrent_halves_of_a_cycle_end_with_one_refusal(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (ma, mb) = (a.module(&pool).await, a.module(&pool).await);
    let mut first = pool.begin().await.expect("begin");
    modules::set_rules(&mut *first, a.scope(), mb, &requires(&[ma]), later())
        .await
        .unwrap();
    let racing = tokio::spawn({
        let pool = pool.clone();
        async move {
            modules::set_rules(&pool, a.scope(), ma, &requires(&[mb]), later())
                .await
                .map(|_| ())
        }
    });
    until_a_call_waits_on_a_lock(&pool).await;
    first.commit().await.expect("commit the first half");
    let second = racing.await.expect("the second write ran");
    assert!(
        matches!(second, Err(Error::PrerequisiteCycle)),
        "{second:?}"
    );
    assert_eq!(prerequisite_rows(&pool, a).await.len(), 1);
}

/// Waits until a session of this test's database waits on a lock.
async fn until_a_call_waits_on_a_lock(pool: &PgPool) {
    for _ in 0..500 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(pool)
        .await
        .expect("read pg_stat_activity");
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no session waited on a lock within 5 s");
}

/// T10.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t10_each_item_kind_takes_its_requirements(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool).await;
    let header = a.item(&pool, module, Kind::Header).await;
    let link = a.item(&pool, module, Kind::ExternalLink).await;
    let page = a.raw_item(&pool, module, "page").await;
    let file = a.raw_item(&pool, module, "file").await;
    let video = a.raw_item(&pool, module, "video").await;
    let watch = Requirement::Watch { min_percent: 80 };
    let allowed = [
        (link, Requirement::View),
        (link, Requirement::MarkDone),
        (page, Requirement::View),
        (page, Requirement::MarkDone),
        (file, Requirement::View),
        (file, Requirement::MarkDone),
        (video, Requirement::View),
        (video, Requirement::MarkDone),
        (video, watch),
    ];
    for (item, requirement) in allowed {
        let stored = items::set_requirement(&pool, a.scope(), item, Some(requirement), later())
            .await
            .unwrap();
        assert_eq!(stored.requirement, Some(requirement));
        let read = items::get(&pool, a.scope(), item).await.unwrap().unwrap();
        assert_eq!(read.requirement, Some(requirement));
    }
    let refused = [
        (header, Requirement::View),
        (header, Requirement::MarkDone),
        (page, watch),
        (file, watch),
        (link, watch),
    ];
    for (item, requirement) in refused {
        let answer =
            items::set_requirement(&pool, a.scope(), item, Some(requirement), later()).await;
        assert!(
            matches!(answer, Err(Error::RequirementInvalid)),
            "{requirement:?}"
        );
    }
    let removed = items::set_requirement(&pool, a.scope(), video, None, later())
        .await
        .unwrap();
    assert_eq!(removed.requirement, None);
    // The table refuses a requirement on a header and a non-object, whatever the writer.
    for value in [r#"{"kind":"view"}"#, r#""view""#] {
        let target = if value.starts_with('{') { header } else { link };
        let written = sqlx::query("UPDATE module_items SET requirement = $2::jsonb WHERE id = $1")
            .bind(target)
            .bind(value)
            .execute(&pool)
            .await;
        assert!(written.is_err(), "{value}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_archived_course_takes_no_rule_write(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (ma, mb) = (a.module(&pool).await, a.module(&pool).await);
    let link = a.item(&pool, ma, Kind::ExternalLink).await;
    for status in [Status::Published, Status::Archived] {
        courses::set_status(&pool, a.scope(), a.course, status, now(), Actor::Instance)
            .await
            .unwrap();
    }
    let refused = set(&pool, a, mb, &requires(&[ma])).await;
    assert!(matches!(refused, Err(Error::CourseArchived)), "{refused:?}");
    let refused =
        items::set_requirement(&pool, a.scope(), link, Some(Requirement::View), later()).await;
    assert!(matches!(refused, Err(Error::CourseArchived)), "{refused:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_modules_and_items_are_not_found(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let theirs = b.module(&pool).await;
    let their_link = b.item(&pool, theirs, Kind::ExternalLink).await;
    let refused = set(&pool, a, theirs, &RulesPatch::default()).await;
    assert!(matches!(refused, Err(Error::ModuleNotFound)), "{refused:?}");
    let refused = items::set_requirement(
        &pool,
        a.scope(),
        their_link,
        Some(Requirement::View),
        later(),
    )
    .await;
    assert!(matches!(refused, Err(Error::ItemNotFound)), "{refused:?}");
    let read = items::get(&pool, b.scope(), their_link)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read.requirement, None);
}

/// The course lock protects positions and the prerequisite graph only at `READ COMMITTED`:
/// every content write in a transaction at another level is refused before it writes.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn content_writes_refuse_isolation_levels_but_read_committed(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (ma, mb) = (a.module(&pool).await, a.module(&pool).await);
    let link = a.item(&pool, ma, Kind::ExternalLink).await;
    let before = prerequisite_rows(&pool, a).await;
    for level in ["REPEATABLE READ", "SERIALIZABLE"] {
        let calls: [&str; 6] = [
            "set_rules",
            "set_requirement",
            "create module",
            "reorder modules",
            "create item",
            "move item",
        ];
        for call in calls {
            let mut tx = pool.begin().await.expect("begin");
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "SET TRANSACTION ISOLATION LEVEL {level}"
            )))
            .execute(&mut *tx)
            .await
            .expect("set the isolation level");
            let s = a.scope();
            let result = match call {
                "set_rules" => modules::set_rules(&mut *tx, s, mb, &requires(&[ma]), later())
                    .await
                    .map(|_| ()),
                "set_requirement" => {
                    items::set_requirement(&mut *tx, s, link, Some(Requirement::View), later())
                        .await
                        .map(|_| ())
                }
                "create module" => modules::create(&mut *tx, s, a.course, "X", later())
                    .await
                    .map(|_| ()),
                "reorder modules" => modules::reorder(&mut *tx, s, a.course, &[mb, ma])
                    .await
                    .map(|_| ()),
                "create item" => items::create(
                    &mut *tx,
                    s,
                    ma,
                    &NewItem {
                        kind: Kind::Header,
                        title: "X".into(),
                        ref_id: None,
                        url: None,
                        position: None,
                    },
                    later(),
                )
                .await
                .map(|_| ()),
                _ => items::move_to(&mut *tx, s, link, mb, 0, later())
                    .await
                    .map(|_| ()),
            };
            assert!(
                matches!(&result, Err(Error::IsolationLevel { found }) if found == &level.to_lowercase()),
                "{level} {call}: {result:?}"
            );
        }
    }
    assert_eq!(prerequisite_rows(&pool, a).await, before);
    assert_eq!(
        modules::list_for_course(&pool, a.scope(), a.course)
            .await
            .unwrap()
            .len(),
        2
    );
}

/// An unlock date sent with nanoseconds is stored to the microsecond, and sending it again
/// writes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unlock_date_with_nanoseconds_compares_to_the_microsecond(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool).await;
    // 2030-01-02T03:04:05.123456789Z.
    let precise = UNIX_EPOCH + Duration::new(1_893_553_445, 123_456_789);
    let patch = RulesPatch {
        unlock_at: Some(Some(precise)),
        ..RulesPatch::default()
    };
    let stored = modules::set_rules(&pool, a.scope(), module, &patch, later())
        .await
        .unwrap();
    assert_eq!(
        stored.unlock_at,
        Some(UNIX_EPOCH + Duration::new(1_893_553_445, 123_456_000))
    );
    let again = modules::set_rules(
        &pool,
        a.scope(),
        module,
        &patch,
        later() + Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert_eq!(again.updated_at, later(), "nothing written");
}
