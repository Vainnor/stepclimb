// SPDX-License-Identifier: AGPL-3.0-or-later
//! The courses repository and the `courses` table against a fresh database per test
//! (docs/specs/m1/02-courses-and-enrollments.md B1 to B11, B27, Data; T3 to T5, T7 at the
//! repository level). Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, accounts,
    accounts::Reach,
    audit::{self, Actor},
    courses::{self, Course, CoursePatch, Error, Home, ListFilter, NewCourse, Status, Visibility},
    id::Uuid,
    organizations,
};

/// 2025-11-13T02:13:20.123456Z.
fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

fn later_time() -> SystemTime {
    request_time() + Duration::from_secs(60)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the credential resolvers"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An organization with the spec's tree: Tower and Enroute under the root.
#[derive(Clone, Copy)]
struct Org {
    id: Uuid,
    root: Uuid,
    tower: Uuid,
    enroute: Uuid,
}

impl Org {
    async fn new(pool: &PgPool, slug: &str) -> Self {
        let id = organizations::create(pool, slug, slug, request_time())
            .await
            .expect("create organization")
            .id;
        let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the root");
        let account = |name| async move {
            accounts::create(pool, scope(id), root, name, request_time(), Actor::Instance)
                .await
                .expect("create account")
                .id
        };
        let tower = account("Tower").await;
        let enroute = account("Enroute").await;
        Self {
            id,
            root,
            tower,
            enroute,
        }
    }

    fn scope(self) -> OrgScope {
        scope(self.id)
    }
}

async fn create(pool: &PgPool, org: Org, account: Uuid, code: &str) -> Result<Course, Error> {
    let new = NewCourse {
        account_id: account,
        title: format!("Course {code}"),
        code: code.into(),
    };
    courses::create(pool, org.scope(), &new, request_time(), Actor::Instance).await
}

async fn entries(pool: &PgPool, org: Org) -> Vec<audit::Entry> {
    audit::list(pool, org.scope(), 100).await.expect("entries")
}

/// Every course row of `org`, as text.
async fn rows(pool: &PgPool, org: Org) -> Vec<String> {
    sqlx::query_scalar("SELECT to_jsonb(c)::text FROM courses c WHERE org_id = $1 ORDER BY id")
        .bind(org.id)
        .fetch_all(pool)
        .await
        .expect("read courses")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_new_course_is_a_draft_with_its_account_path_and_one_entry(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let new = NewCourse {
        account_id: a.tower,
        title: "  S1 Ground  ".into(),
        code: " S1 ".into(),
    };
    let course = courses::create(&pool, a.scope(), &new, request_time(), Actor::Instance)
        .await
        .unwrap();
    assert_eq!(
        (course.title.as_str(), course.code.as_str()),
        ("S1 Ground", "S1")
    );
    assert_eq!(course.status, Status::Draft);
    assert!(!course.template);
    assert_eq!(course.account_path, [a.root, a.tower]);
    assert_eq!(course.settings.home, Home::Modules);
    assert_eq!((course.start_at, course.end_at), (None, None));
    assert_eq!(course.created_at, request_time());
    assert_eq!(
        courses::get(&pool, a.scope(), course.id).await.unwrap(),
        Some(course.clone())
    );
    let log = entries(&pool, a).await;
    let created: Vec<_> = log.iter().filter(|e| e.action == "course.create").collect();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].target_id, course.id);
    assert_eq!(
        created[0].data,
        json!({ "after": {
            "account_id": a.tower, "title": "S1 Ground", "code": "S1", "template": false,
        } })
    );
}

/// T3.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_codes_are_unique_per_organization_ignoring_ascii_case(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    create(&pool, a, a.tower, "S1").await.unwrap();
    assert!(matches!(
        create(&pool, a, a.enroute, "s1").await,
        Err(Error::CodeTaken)
    ));
    create(&pool, b, b.tower, "S1").await.unwrap();
    assert_eq!(rows(&pool, a).await.len(), 1);
    let created = entries(&pool, a).await;
    assert_eq!(
        created
            .iter()
            .filter(|e| e.action == "course.create")
            .count(),
        1,
        "a refused create writes no entry"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_account_and_course_are_not_found(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    assert!(matches!(
        create(&pool, a, b.tower, "S1").await,
        Err(Error::AccountNotFound)
    ));
    let theirs = create(&pool, b, b.tower, "S1").await.unwrap();
    let before = rows(&pool, b).await;
    assert_eq!(
        courses::get(&pool, a.scope(), theirs.id).await.unwrap(),
        None
    );
    assert_eq!(
        courses::path(&pool, a.scope(), theirs.id).await.unwrap(),
        None
    );
    let patch = CoursePatch {
        title: Some("Mine".into()),
        ..CoursePatch::default()
    };
    let now = later_time();
    let refused = [
        courses::update(&pool, a.scope(), theirs.id, &patch, now, Actor::Instance).await,
        courses::set_status(
            &pool,
            a.scope(),
            theirs.id,
            Status::Published,
            now,
            Actor::Instance,
        )
        .await,
        courses::move_to(&pool, a.scope(), theirs.id, a.tower, now, Actor::Instance).await,
    ];
    for result in refused {
        assert!(matches!(result, Err(Error::NotFound)), "{result:?}");
    }
    assert!(matches!(
        courses::move_to(&pool, b.scope(), theirs.id, a.tower, now, Actor::Instance).await,
        Err(Error::AccountNotFound)
    ));
    assert_eq!(rows(&pool, b).await, before);
}

/// T4 at the repository level.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_an_update_writes_changed_members_and_a_no_op_writes_nothing(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = create(&pool, a, a.tower, "S1").await.unwrap();
    let same = CoursePatch {
        title: Some(course.title.clone()),
        code: Some("S1".into()),
        start_at: Some(None),
        home: Some(Home::Modules),
        ..CoursePatch::default()
    };
    let unchanged = courses::update(
        &pool,
        a.scope(),
        course.id,
        &same,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(unchanged, course);
    assert!(
        !entries(&pool, a)
            .await
            .iter()
            .any(|e| e.action == "course.update")
    );

    let start = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let patch = CoursePatch {
        code: Some("S1-2026".into()),
        start_at: Some(Some(start)),
        ..CoursePatch::default()
    };
    let updated = courses::update(
        &pool,
        a.scope(),
        course.id,
        &patch,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(updated.code, "S1-2026");
    assert_eq!(updated.start_at, Some(start));
    assert_eq!(updated.updated_at, later_time());
    let log = entries(&pool, a).await;
    let update: Vec<_> = log.iter().filter(|e| e.action == "course.update").collect();
    assert_eq!(update.len(), 1);
    assert_eq!(
        update[0].data,
        json!({
            "before": { "code": "S1", "start_at": null },
            "after": { "code": "S1-2026", "start_at": "2027-01-15T08:00:00Z" },
        })
    );

    let inverted = CoursePatch {
        end_at: Some(Some(start - Duration::from_secs(1))),
        ..CoursePatch::default()
    };
    assert!(matches!(
        courses::update(
            &pool,
            a.scope(),
            course.id,
            &inverted,
            later_time(),
            Actor::Instance
        )
        .await,
        Err(Error::DatesInverted)
    ));
    let cleared = CoursePatch {
        start_at: Some(None),
        ..CoursePatch::default()
    };
    let cleared = courses::update(
        &pool,
        a.scope(),
        course.id,
        &cleared,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(cleared.start_at, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_update_to_a_code_taken_ignoring_case_is_refused(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    create(&pool, a, a.tower, "S1").await.unwrap();
    let other = create(&pool, a, a.tower, "S2").await.unwrap();
    let patch = CoursePatch {
        code: Some("s1".into()),
        ..CoursePatch::default()
    };
    assert!(matches!(
        courses::update(
            &pool,
            a.scope(),
            other.id,
            &patch,
            later_time(),
            Actor::Instance
        )
        .await,
        Err(Error::CodeTaken)
    ));
}

/// T5 at the repository level, without enrollments.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_status_changes_follow_b4(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = create(&pool, a, a.tower, "S1").await.unwrap();
    let set = |status| {
        let pool = pool.clone();
        async move {
            courses::set_status(
                &pool,
                a.scope(),
                course.id,
                status,
                later_time(),
                Actor::Instance,
            )
            .await
        }
    };
    assert!(matches!(
        set(Status::Archived).await,
        Err(Error::StatusInvalid {
            from: "draft",
            to: "archived"
        })
    ));
    assert_eq!(set(Status::Draft).await.unwrap().status, Status::Draft);
    for status in [
        Status::Published,
        Status::Archived,
        Status::Published,
        Status::Draft,
    ] {
        assert_eq!(set(status).await.unwrap().status, status);
    }
    let log = entries(&pool, a).await;
    let changes: Vec<_> = log
        .iter()
        .filter(|e| e.action == "course.set_status")
        .map(|e| e.data.clone())
        .collect();
    assert_eq!(changes.len(), 4, "the refused and the unchanged write none");
    assert_eq!(
        changes[0],
        json!({ "before": { "status": "draft" }, "after": { "status": "published" } })
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_template_cannot_be_published(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = create(&pool, a, a.tower, "T1").await.unwrap();
    sqlx::query("UPDATE courses SET template = true WHERE id = $1")
        .bind(course.id)
        .execute(&pool)
        .await
        .unwrap();
    let published = courses::set_status(
        &pool,
        a.scope(),
        course.id,
        Status::Published,
        later_time(),
        Actor::Instance,
    )
    .await;
    assert!(matches!(published, Err(Error::Template)));
    // The table refuses it too.
    let stored = sqlx::query("UPDATE courses SET status = 'published' WHERE id = $1")
        .bind(course.id)
        .execute(&pool)
        .await;
    assert!(stored.is_err());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_changes_the_account_and_a_move_in_place_writes_nothing(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = create(&pool, a, a.tower, "S1").await.unwrap();
    let same = courses::move_to(
        &pool,
        a.scope(),
        course.id,
        a.tower,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(same, course);
    let moved = courses::move_to(
        &pool,
        a.scope(),
        course.id,
        a.enroute,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(moved.account_path, [a.root, a.enroute]);
    assert_eq!(moved.updated_at, later_time());
    let log = entries(&pool, a).await;
    let moves: Vec<_> = log.iter().filter(|e| e.action == "course.move").collect();
    assert_eq!(moves.len(), 1);
    assert_eq!(
        moves[0].data,
        json!({ "before": { "account_id": a.tower }, "after": { "account_id": a.enroute } })
    );
}

/// T7 at the repository level, and B9's account move carrying its courses.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_an_account_holding_a_course_cannot_be_deleted(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = create(&pool, a, a.tower, "S1").await.unwrap();
    courses::set_status(
        &pool,
        a.scope(),
        course.id,
        Status::Published,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    courses::set_status(
        &pool,
        a.scope(),
        course.id,
        Status::Archived,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let refused = accounts::delete(&pool, a.scope(), a.tower, later_time(), Actor::Instance).await;
    assert!(
        matches!(refused, Err(accounts::Error::HasCourses)),
        "{refused:?}"
    );
    accounts::move_to(
        &pool,
        a.scope(),
        a.tower,
        a.enroute,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(
        courses::path(&pool, a.scope(), course.id).await.unwrap(),
        Some(vec![a.root, a.enroute, a.tower])
    );
    courses::move_to(
        &pool,
        a.scope(),
        course.id,
        a.root,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    accounts::delete(&pool, a.scope(), a.tower, later_time(), Actor::Instance)
        .await
        .unwrap();
}

/// B10, B11: the list's reach, its draft rule and its filters.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_list_shows_drafts_only_where_view_all_reaches(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let in_tower = create(&pool, a, a.tower, "B").await.unwrap();
    let draft_in_tower = create(&pool, a, a.tower, "A").await.unwrap();
    let in_enroute = create(&pool, a, a.enroute, "C").await.unwrap();
    create(&pool, b, b.tower, "B").await.unwrap();
    for id in [in_tower.id, in_enroute.id] {
        courses::set_status(
            &pool,
            a.scope(),
            id,
            Status::Published,
            later_time(),
            Actor::Instance,
        )
        .await
        .unwrap();
    }
    let list = |visibility, filter| {
        let pool = pool.clone();
        async move {
            courses::list_visible(&pool, a.scope(), visibility, filter, None, 50)
                .await
                .unwrap()
                .into_iter()
                .map(|c| c.id)
                .collect::<Vec<_>>()
        }
    };
    let everything = Visibility {
        view: Reach::Organization,
        view_all: Reach::Organization,
    };
    assert_eq!(
        list(everything, ListFilter::default()).await,
        [draft_in_tower.id, in_tower.id, in_enroute.id],
        "sorted by title"
    );
    let tower = [a.tower];
    let learner = Visibility {
        view: Reach::Organization,
        view_all: Reach::Accounts(&[]),
    };
    assert_eq!(
        list(learner, ListFilter::default()).await,
        [in_tower.id, in_enroute.id]
    );
    let tower_staff = Visibility {
        view: Reach::Accounts(&tower),
        view_all: Reach::Accounts(&tower),
    };
    assert_eq!(
        list(tower_staff, ListFilter::default()).await,
        [draft_in_tower.id, in_tower.id]
    );
    let in_enroute_only = ListFilter {
        account_id: Some(a.enroute),
        ..ListFilter::default()
    };
    assert_eq!(list(everything, in_enroute_only).await, [in_enroute.id]);
    let drafts = ListFilter {
        status: Some(Status::Draft),
        ..ListFilter::default()
    };
    assert_eq!(list(everything, drafts).await, [draft_in_tower.id]);
    let templates = ListFilter {
        template: Some(true),
        ..ListFilter::default()
    };
    assert!(list(everything, templates).await.is_empty());
    let after = (draft_in_tower.title.clone(), draft_in_tower.id);
    let page = courses::list_visible(
        &pool,
        a.scope(),
        everything,
        ListFilter::default(),
        Some(&after),
        1,
    )
    .await
    .unwrap();
    assert_eq!(page.iter().map(|c| c.id).collect::<Vec<_>>(), [in_tower.id]);
}
