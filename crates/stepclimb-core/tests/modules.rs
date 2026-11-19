// SPDX-License-Identifier: AGPL-3.0-or-later
//! The modules and items repositories against a fresh database per test
//! (docs/specs/m1/03-modules-and-items.md B1 to B12, Data; T1, T3, T4, T6 to T8 at the
//! repository level). Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    courses::{self, NewCourse, Status},
    id::{Uuid, new_id},
    items::{self, ItemPatch, Kind, NewItem},
    modules::{self, Error, ModulePatch},
    organizations,
};

/// 2025-11-13T02:13:20.123456Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
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
        let org = Self {
            id,
            course: Uuid::nil(),
        };
        let course = org.course(pool, "C1").await;
        Self { id, course }
    }

    fn scope(self) -> OrgScope {
        scope(self.id)
    }

    /// A new course in the root account with the code `code`.
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

    async fn module(self, pool: &PgPool, title: &str) -> Uuid {
        modules::create(pool, self.scope(), self.course, title, now())
            .await
            .expect("create module")
            .id
    }

    async fn header(self, pool: &PgPool, module: Uuid, title: &str) -> Uuid {
        items::create(pool, self.scope(), module, &header(title), now())
            .await
            .expect("create item")
            .id
    }

    async fn archive(self, pool: &PgPool) {
        for status in [Status::Published, Status::Archived] {
            courses::set_status(
                pool,
                self.scope(),
                self.course,
                status,
                now(),
                Actor::Instance,
            )
            .await
            .expect("change status");
        }
    }
}

fn header(title: &str) -> NewItem {
    NewItem {
        kind: Kind::Header,
        title: title.into(),
        ref_id: None,
        url: None,
        position: None,
    }
}

/// The module ids of the course, in order, checking positions are 0, 1, 2, ...
async fn module_order(pool: &PgPool, org: Org) -> Vec<Uuid> {
    let list = modules::list_for_course(pool, org.scope(), org.course)
        .await
        .expect("list modules");
    let positions: Vec<i32> = list.iter().map(|m| m.position).collect();
    assert_eq!(positions, (0..).take(list.len()).collect::<Vec<_>>());
    list.into_iter().map(|m| m.id).collect()
}

/// The item ids of the module, in order, checking positions are 0, 1, 2, ...
async fn item_order(pool: &PgPool, org: Org, module: Uuid) -> Vec<Uuid> {
    let list = items::list_for_module(pool, org.scope(), module)
        .await
        .expect("list items");
    let positions: Vec<i32> = list.iter().map(|i| i.position).collect();
    assert_eq!(positions, (0..).take(list.len()).collect::<Vec<_>>());
    list.into_iter().map(|i| i.id).collect()
}

/// Every module and item row of `org`, as text.
async fn rows(pool: &PgPool, org: Org) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT to_jsonb(m)::text FROM modules m WHERE org_id = $1
         UNION ALL
         SELECT to_jsonb(i)::text FROM module_items i WHERE org_id = $1
         ORDER BY 1",
    )
    .bind(org.id)
    .fetch_all(pool)
    .await
    .expect("read rows")
}

// Modules (B1 to B3).

/// T1.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_modules_go_last_unpublished_up_to_the_limit(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let first = modules::create(&pool, a.scope(), a.course, "  Week 1 ", now())
        .await
        .unwrap();
    assert_eq!(first.title, "Week 1");
    assert_eq!((first.position, first.published), (0, false));
    assert_eq!((first.created_at, first.updated_at), (now(), now()));
    let second = a.module(&pool, "Week 2").await;
    let third = a.module(&pool, "Week 3").await;
    assert_eq!(module_order(&pool, a).await, [first.id, second, third]);
    for n in 3..modules::MAX_PER_COURSE {
        a.module(&pool, &format!("Week {n}")).await;
    }
    let over = modules::create(&pool, a.scope(), a.course, "One too many", now()).await;
    assert!(matches!(over, Err(Error::ModuleLimitReached)), "{over:?}");
    assert_eq!(module_order(&pool, a).await.len(), 200);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_module_patch_renames_and_publishes_and_a_same_value_patch_writes_nothing(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let id = a.module(&pool, "Week 1").await;
    let same = ModulePatch {
        title: Some(" Week 1 ".into()),
        published: Some(false),
        completion_valid_months: Some(None),
    };
    let unchanged = modules::update(&pool, a.scope(), id, &same, later())
        .await
        .unwrap();
    assert_eq!(unchanged.updated_at, now(), "nothing written");
    let patch = ModulePatch {
        title: Some("Week one".into()),
        published: Some(true),
        completion_valid_months: None,
    };
    let changed = modules::update(&pool, a.scope(), id, &patch, later())
        .await
        .unwrap();
    assert_eq!(
        (changed.title.as_str(), changed.published),
        ("Week one", true)
    );
    assert_eq!(changed.updated_at, later());
    let blank = ModulePatch {
        title: Some(" ".into()),
        published: None,
        completion_valid_months: None,
    };
    let refused = modules::update(&pool, a.scope(), id, &blank, later()).await;
    assert!(matches!(refused, Err(Error::TitleEmpty)), "{refused:?}");
}

/// T3.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_only_an_empty_module_deletes_and_the_gap_closes(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let first = a.module(&pool, "Week 1").await;
    let middle = a.module(&pool, "Week 2").await;
    let last = a.module(&pool, "Week 3").await;
    let item = a.header(&pool, middle, "Read this").await;
    let refused = modules::delete(&pool, a.scope(), middle).await;
    assert!(matches!(refused, Err(Error::ModuleNotEmpty)), "{refused:?}");
    items::delete(&pool, a.scope(), item).await.unwrap();
    modules::delete(&pool, a.scope(), middle).await.unwrap();
    assert_eq!(module_order(&pool, a).await, [first, last]);
    let gone = modules::delete(&pool, a.scope(), middle).await;
    assert!(matches!(gone, Err(Error::ModuleNotFound)), "{gone:?}");
}

// Items (B4 to B9).

/// T4.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_only_headers_and_external_links_are_created(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool, "Week 1").await;
    for kind in [
        Kind::Quiz,
        Kind::Activity,
        Kind::Page,
        Kind::File,
        Kind::Video,
    ] {
        let new = NewItem {
            ref_id: Some(new_id()),
            ..NewItem {
                kind,
                ..header("X")
            }
        };
        let refused = items::create(&pool, a.scope(), module, &new, now()).await;
        assert!(matches!(refused, Err(Error::KindUnavailable)), "{kind:?}");
    }
    let link = NewItem {
        kind: Kind::ExternalLink,
        url: Some("https://example.com/a?b=1".into()),
        ..header("Charts")
    };
    let link = items::create(&pool, a.scope(), module, &link, now())
        .await
        .unwrap();
    assert_eq!(link.url.as_deref(), Some("https://example.com/a?b=1"));
    assert_eq!((link.kind, link.ref_id), (Kind::ExternalLink, None));
    assert_eq!((link.course_id, link.module_id), (a.course, module));
    assert_eq!((link.position, link.published), (0, false));
    let header = a.header(&pool, module, "Section").await;
    assert_eq!(item_order(&pool, a, module).await, [link.id, header]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_item_goes_at_its_position_and_the_module_holds_200(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool, "Week 1").await;
    let x = a.header(&pool, module, "X").await;
    let y = a.header(&pool, module, "Y").await;
    let at = |position| NewItem {
        position: Some(position),
        ..header("Z")
    };
    let first = items::create(&pool, a.scope(), module, &at(0), now())
        .await
        .unwrap()
        .id;
    let middle = items::create(&pool, a.scope(), module, &at(2), now())
        .await
        .unwrap()
        .id;
    let last = items::create(&pool, a.scope(), module, &at(4), now())
        .await
        .unwrap()
        .id;
    assert_eq!(
        item_order(&pool, a, module).await,
        [first, x, middle, y, last]
    );
    for bad in [-1, 6] {
        let refused = items::create(&pool, a.scope(), module, &at(bad), now()).await;
        assert!(matches!(refused, Err(Error::PositionOutOfRange)), "{bad}");
    }
    for n in 5..items::MAX_PER_MODULE {
        a.header(&pool, module, &format!("H{n}")).await;
    }
    let over = items::create(&pool, a.scope(), module, &header("Over"), now()).await;
    assert!(matches!(over, Err(Error::ItemLimitReached)), "{over:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_item_patch_changes_title_publish_and_a_links_url_only(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool, "Week 1").await;
    let header_id = a.header(&pool, module, "Section").await;
    let url_on_header = ItemPatch {
        url: Some("https://example.com/".into()),
        ..ItemPatch::default()
    };
    let refused = items::update(&pool, a.scope(), header_id, &url_on_header, later()).await;
    assert!(matches!(refused, Err(Error::UrlUnavailable)), "{refused:?}");
    let new = NewItem {
        kind: Kind::ExternalLink,
        url: Some("https://example.com/old".into()),
        ..header("Link")
    };
    let link = items::create(&pool, a.scope(), module, &new, now())
        .await
        .unwrap();
    for bad in ["javascript:alert(1)", "https://example.com/\u{202E}gpj.exe"] {
        let patch = ItemPatch {
            url: Some(bad.into()),
            ..ItemPatch::default()
        };
        let refused = items::update(&pool, a.scope(), link.id, &patch, later()).await;
        assert!(
            matches!(refused, Err(Error::UrlInvalid)),
            "{bad:?}: {refused:?}"
        );
    }
    let same = ItemPatch {
        title: Some("Link".into()),
        published: Some(false),
        url: Some("https://example.com/old".into()),
    };
    let unchanged = items::update(&pool, a.scope(), link.id, &same, later())
        .await
        .unwrap();
    assert_eq!(unchanged.updated_at, now(), "nothing written");
    let patch = ItemPatch {
        title: Some("New link".into()),
        published: Some(true),
        url: Some("https://example.com/new".into()),
    };
    let changed = items::update(&pool, a.scope(), link.id, &patch, later())
        .await
        .unwrap();
    assert_eq!(changed.url.as_deref(), Some("https://example.com/new"));
    assert_eq!(
        (changed.title.as_str(), changed.published),
        ("New link", true)
    );
    assert_eq!(changed.updated_at, later());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn deleting_an_item_closes_the_gap(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool, "Week 1").await;
    let x = a.header(&pool, module, "X").await;
    let y = a.header(&pool, module, "Y").await;
    let z = a.header(&pool, module, "Z").await;
    items::delete(&pool, a.scope(), y).await.unwrap();
    assert_eq!(item_order(&pool, a, module).await, [x, z]);
    let gone = items::delete(&pool, a.scope(), y).await;
    assert!(matches!(gone, Err(Error::ItemNotFound)), "{gone:?}");
}

// Ordering (B10 to B12).

/// T6.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_a_reorder_takes_exactly_the_current_set(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let (m1, m2, m3) = (
        a.module(&pool, "1").await,
        a.module(&pool, "2").await,
        a.module(&pool, "3").await,
    );
    let other_course = a.course(&pool, "C2").await;
    let elsewhere = modules::create(&pool, a.scope(), other_course, "Other", now())
        .await
        .unwrap()
        .id;
    let theirs = b.module(&pool, "Theirs").await;
    let before = rows(&pool, a).await;
    for stale in [
        vec![m3, m1],
        vec![m3, m1, m2, new_id()],
        vec![m3, m1, m1],
        vec![m3, m1, m2, m2],
        vec![m3, m1, elsewhere],
        vec![m3, m1, theirs],
    ] {
        let refused = modules::reorder(&pool, a.scope(), a.course, &stale).await;
        assert!(matches!(refused, Err(Error::OrderStale)), "{stale:?}");
    }
    assert_eq!(rows(&pool, a).await, before, "nothing changed");
    let same = modules::reorder(&pool, a.scope(), a.course, &[m1, m2, m3])
        .await
        .unwrap();
    assert_eq!(same.len(), 3);
    assert_eq!(
        rows(&pool, a).await,
        before,
        "the current order writes nothing"
    );
    let reordered = modules::reorder(&pool, a.scope(), a.course, &[m3, m1, m2])
        .await
        .unwrap();
    let ids: Vec<Uuid> = reordered.iter().map(|m| m.id).collect();
    assert_eq!(ids, [m3, m1, m2]);
    assert_eq!(module_order(&pool, a).await, [m3, m1, m2]);

    let (x, y) = (
        a.header(&pool, m1, "X").await,
        a.header(&pool, m1, "Y").await,
    );
    let refused = items::reorder(&pool, a.scope(), m1, &[y]).await;
    assert!(matches!(refused, Err(Error::OrderStale)), "{refused:?}");
    items::reorder(&pool, a.scope(), m1, &[y, x]).await.unwrap();
    assert_eq!(item_order(&pool, a, m1).await, [y, x]);
}

/// T7.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_a_move_keeps_both_modules_dense_and_stays_in_the_course(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (ma, mb) = (a.module(&pool, "A").await, a.module(&pool, "B").await);
    let (a0, a1, a2) = (
        a.header(&pool, ma, "a0").await,
        a.header(&pool, ma, "a1").await,
        a.header(&pool, ma, "a2").await,
    );
    let b0 = a.header(&pool, mb, "b0").await;
    let moved = items::move_to(&pool, a.scope(), a1, mb, 0, later())
        .await
        .unwrap();
    assert_eq!((moved.module_id, moved.position), (mb, 0));
    assert_eq!(moved.updated_at, later());
    assert_eq!(item_order(&pool, a, ma).await, [a0, a2]);
    assert_eq!(item_order(&pool, a, mb).await, [a1, b0]);

    // Within one module, both directions, and the item's own place.
    items::move_to(&pool, a.scope(), b0, mb, 0, later())
        .await
        .unwrap();
    assert_eq!(item_order(&pool, a, mb).await, [b0, a1]);
    items::move_to(&pool, a.scope(), b0, mb, 1, later())
        .await
        .unwrap();
    assert_eq!(item_order(&pool, a, mb).await, [a1, b0]);
    let before = rows(&pool, a).await;
    items::move_to(&pool, a.scope(), b0, mb, 1, later())
        .await
        .unwrap();
    assert_eq!(rows(&pool, a).await, before, "its own place writes nothing");

    for bad in [-1, 2] {
        let refused = items::move_to(&pool, a.scope(), b0, mb, bad, later()).await;
        assert!(matches!(refused, Err(Error::PositionOutOfRange)), "{bad}");
    }
    let refused = items::move_to(&pool, a.scope(), a0, mb, 3, later()).await;
    assert!(
        matches!(refused, Err(Error::PositionOutOfRange)),
        "{refused:?}"
    );
    items::move_to(&pool, a.scope(), a0, mb, 2, later())
        .await
        .unwrap();
    assert_eq!(item_order(&pool, a, mb).await, [a1, b0, a0]);

    let other_course = a.course(&pool, "C2").await;
    let elsewhere = modules::create(&pool, a.scope(), other_course, "Other", now())
        .await
        .unwrap()
        .id;
    let before = rows(&pool, a).await;
    let refused = items::move_to(&pool, a.scope(), a2, elsewhere, 0, later()).await;
    assert!(matches!(refused, Err(Error::ModuleNotFound)), "{refused:?}");
    assert_eq!(rows(&pool, a).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_into_a_full_module_is_refused(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (ma, mb) = (a.module(&pool, "A").await, a.module(&pool, "B").await);
    let item = a.header(&pool, ma, "x").await;
    for n in 0..items::MAX_PER_MODULE {
        a.header(&pool, mb, &format!("H{n}")).await;
    }
    let refused = items::move_to(&pool, a.scope(), item, mb, 0, later()).await;
    assert!(
        matches!(refused, Err(Error::ItemLimitReached)),
        "{refused:?}"
    );
}

/// T8.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_twenty_concurrent_creates_end_dense(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool, "Week 1").await;
    let tasks: Vec<_> = (0..20)
        .map(|n| {
            let pool = pool.clone();
            tokio::spawn(async move {
                let mut conn = pool.acquire().await.expect("a connection");
                let new = NewItem {
                    // Half go first, half last, so the inserts shift each other.
                    position: (n % 2 == 0).then_some(0),
                    ..header(&format!("H{n}"))
                };
                items::create(&mut *conn, a.scope(), module, &new, now()).await
            })
        })
        .collect();
    for task in tasks {
        task.await.expect("the task ran").expect("the create");
    }
    assert_eq!(item_order(&pool, a, module).await.len(), 20);
}

/// T8 for modules, and a reorder racing a create.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_reorder_waiting_on_a_create_sees_the_new_module_and_is_stale(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (m1, m2) = (a.module(&pool, "1").await, a.module(&pool, "2").await);
    let mut creating = pool.begin().await.expect("begin");
    modules::create(&mut *creating, a.scope(), a.course, "3", now())
        .await
        .unwrap();
    let racing = tokio::spawn({
        let pool = pool.clone();
        async move { modules::reorder(&pool, a.scope(), a.course, &[m2, m1]).await }
    });
    until_a_call_waits_on_a_lock(&pool).await;
    creating.commit().await.expect("commit the create");
    let answer = racing.await.expect("the reorder ran");
    assert!(matches!(answer, Err(Error::OrderStale)), "{answer:?}");
    assert_eq!(module_order(&pool, a).await.len(), 3);
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

// The course (spec 02 B5) and the organization.

/// T2 at the repository level.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_an_archived_course_takes_no_content_write(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let (m1, m2) = (a.module(&pool, "1").await, a.module(&pool, "2").await);
    let item = a.header(&pool, m1, "X").await;
    a.archive(&pool).await;
    let before = rows(&pool, a).await;
    let s = a.scope();
    let results = [
        modules::create(&pool, s, a.course, "3", now()).await.err(),
        modules::update(&pool, s, m1, &ModulePatch::default(), now())
            .await
            .err(),
        modules::delete(&pool, s, m2).await.err(),
        modules::reorder(&pool, s, a.course, &[m2, m1]).await.err(),
        items::create(&pool, s, m1, &header("Y"), now()).await.err(),
        items::update(&pool, s, item, &ItemPatch::default(), now())
            .await
            .err(),
        items::reorder(&pool, s, m1, &[item]).await.err(),
        items::move_to(&pool, s, item, m2, 0, now()).await.err(),
        items::delete(&pool, s, item).await.err(),
    ];
    for (n, result) in results.into_iter().enumerate() {
        assert!(
            matches!(result, Some(Error::CourseArchived)),
            "{n}: {result:?}"
        );
    }
    assert_eq!(rows(&pool, a).await, before);
    assert_eq!(
        modules::list_for_course(&pool, s, a.course)
            .await
            .unwrap()
            .len(),
        2,
        "reads still work"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_ids_are_not_found(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let module = b.module(&pool, "Theirs").await;
    let item = b.header(&pool, module, "X").await;
    let mine = a.module(&pool, "Mine").await;
    let before = rows(&pool, b).await;
    let s = a.scope();
    assert!(modules::get(&pool, s, module).await.unwrap().is_none());
    assert!(
        modules::course_of(&pool, s, module)
            .await
            .unwrap()
            .is_none()
    );
    assert!(items::get(&pool, s, item).await.unwrap().is_none());
    assert!(items::course_of(&pool, s, item).await.unwrap().is_none());
    let created = modules::create(&pool, s, b.course, "X", now()).await;
    assert!(matches!(created, Err(Error::CourseNotFound)), "{created:?}");
    let patched = modules::update(&pool, s, module, &ModulePatch::default(), now()).await;
    assert!(matches!(patched, Err(Error::ModuleNotFound)), "{patched:?}");
    let added = items::create(&pool, s, module, &header("Y"), now()).await;
    assert!(matches!(added, Err(Error::ModuleNotFound)), "{added:?}");
    let moved = items::move_to(&pool, s, item, mine, 0, now()).await;
    assert!(matches!(moved, Err(Error::ItemNotFound)), "{moved:?}");
    let deleted = items::delete(&pool, s, item).await;
    assert!(matches!(deleted, Err(Error::ItemNotFound)), "{deleted:?}");
    assert_eq!(rows(&pool, b).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_tables_refuse_a_kind_without_its_url_a_non_http_url_or_a_reference(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let module = a.module(&pool, "Week 1").await;
    let insert = |kind: &'static str, url: Option<&'static str>, ref_id: Option<Uuid>| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind,
                                           url, ref_id, title)
                 VALUES ($1, $2, $3, $4, 99, $5, $6, $7, 'T')",
            )
            .bind(new_id())
            .bind(a.id)
            .bind(a.course)
            .bind(module)
            .bind(kind)
            .bind(url)
            .bind(ref_id)
            .execute(&pool)
            .await
        }
    };
    assert!(insert("external_link", None, None).await.is_err());
    // The backstop under `items::url_is_valid`: only http and https.
    assert!(
        insert("external_link", Some("javascript:alert(1)"), None)
            .await
            .is_err()
    );
    assert!(
        insert("external_link", Some("HTTPS://x.example/"), None)
            .await
            .is_ok()
    );
    assert!(
        insert("header", Some("https://x.example/"), None)
            .await
            .is_err()
    );
    assert!(insert("header", None, Some(new_id())).await.is_err());
    assert!(insert("page", None, None).await.is_err());
    assert!(insert("lti", None, None).await.is_err());
    // An item's course must be its module's.
    let other = a.course(&pool, "C2").await;
    let drifted = sqlx::query(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, title)
         VALUES ($1, $2, $3, $4, 0, 'header', 'T')",
    )
    .bind(new_id())
    .bind(a.id)
    .bind(other)
    .bind(module)
    .execute(&pool)
    .await;
    assert!(drifted.is_err());
}
