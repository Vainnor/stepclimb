// SPDX-License-Identifier: AGPL-3.0-or-later
//! The courses repository and the `courses` table against a fresh database per test
//! (docs/specs/m1/02-courses-and-enrollments.md B1 to B14, B27, Data; T3 to T5, T7, T10 at the
//! repository level). Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, accounts,
    accounts::Reach,
    audit::{self, Actor},
    courses::{
        self, Course, CourseCopy, CoursePatch, Error, Home, ListFilter, NewCourse, Status,
        Visibility,
    },
    id::Uuid,
    items::{self, Item, Kind, NewItem},
    modules::{self, Module, ModulePatch, RulesPatch},
    organizations,
    rules::{CompletionRule, Requirement},
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
        view_courses: &[],
        view_all: Reach::Organization,
        view_all_courses: &[],
    };
    assert_eq!(
        list(everything, ListFilter::default()).await,
        [draft_in_tower.id, in_tower.id, in_enroute.id],
        "sorted by title"
    );
    let tower = [a.tower];
    let learner = Visibility {
        view: Reach::Organization,
        view_courses: &[],
        view_all: Reach::Accounts(&[]),
        view_all_courses: &[],
    };
    assert_eq!(
        list(learner, ListFilter::default()).await,
        [in_tower.id, in_enroute.id]
    );
    let tower_staff = Visibility {
        view: Reach::Accounts(&tower),
        view_courses: &[],
        view_all: Reach::Accounts(&tower),
        view_all_courses: &[],
    };
    assert_eq!(
        list(tower_staff, ListFilter::default()).await,
        [draft_in_tower.id, in_tower.id]
    );
    // Enrollments reach their courses only, and a draft still needs `course.view_all` (B11).
    let enrolled = [draft_in_tower.id, in_enroute.id];
    let student = Visibility {
        view: Reach::Accounts(&[]),
        view_courses: &enrolled,
        view_all: Reach::Accounts(&[]),
        view_all_courses: &[],
    };
    assert_eq!(list(student, ListFilter::default()).await, [in_enroute.id]);
    let designer = Visibility {
        view_all_courses: &enrolled,
        ..student
    };
    assert_eq!(
        list(designer, ListFilter::default()).await,
        [draft_in_tower.id, in_enroute.id]
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

// Copying (B12 to B14).

fn copy_to(account: Uuid, code: &str) -> CourseCopy {
    CourseCopy {
        account_id: account,
        title: None,
        code: code.into(),
        template: false,
    }
}

/// T10 at the repository level: the copy is a new draft in the named account with the given
/// code, the source's title, dates and settings, and one `course.copy` entry on the copy; the
/// source is unchanged.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_copy_is_a_draft_with_the_sources_details_and_one_entry(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap();
    let start = UNIX_EPOCH + Duration::from_secs(1_767_225_600);
    let patch = CoursePatch {
        start_at: Some(Some(start)),
        end_at: Some(Some(start + Duration::from_secs(86_400))),
        ..CoursePatch::default()
    };
    let source = courses::update(
        &pool,
        a.scope(),
        source.id,
        &patch,
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    for status in [Status::Published, Status::Archived] {
        courses::set_status(
            &pool,
            a.scope(),
            source.id,
            status,
            request_time(),
            Actor::Instance,
        )
        .await
        .unwrap();
    }
    let source = courses::get(&pool, a.scope(), source.id)
        .await
        .unwrap()
        .unwrap();
    let before = entries(&pool, a).await.len();

    let copy = courses::copy(
        &pool,
        a.scope(),
        source.id,
        &copy_to(a.enroute, " S1-COPY "),
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();

    assert_ne!(copy.id, source.id);
    assert_eq!(copy.account_id, a.enroute);
    assert_eq!(copy.account_path, [a.root, a.enroute]);
    assert_eq!(copy.code, "S1-COPY");
    assert_eq!(copy.title, source.title);
    assert_eq!(
        copy.status,
        Status::Draft,
        "a copy of an archived course is a draft"
    );
    assert!(!copy.template);
    assert_eq!(
        (copy.start_at, copy.end_at),
        (source.start_at, source.end_at)
    );
    assert_eq!(copy.settings, source.settings);
    assert_eq!(
        (copy.created_at, copy.updated_at),
        (later_time(), later_time())
    );
    assert_eq!(
        courses::get(&pool, a.scope(), source.id)
            .await
            .unwrap()
            .unwrap(),
        source,
        "the source is unchanged"
    );
    let log = entries(&pool, a).await;
    let new: Vec<_> = log.iter().skip(before).collect();
    assert_eq!(new.len(), 1, "{new:#?}");
    assert_eq!(new[0].action, "course.copy");
    assert_eq!(new[0].target_id, copy.id);
    assert_eq!(
        new[0].data,
        json!({ "after": { "source_id": source.id, "account_id": a.enroute, "template": false } })
    );
}

/// B12: a given title replaces the source's; `template` makes a template, and copying a
/// template without it makes an ordinary course; a template copy cannot be published (Q5).
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_copy_takes_a_title_and_the_template_flag(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap();
    let as_template = CourseCopy {
        title: Some("  S1 template ".into()),
        template: true,
        ..copy_to(a.tower, "T1")
    };

    let template = courses::copy(
        &pool,
        a.scope(),
        source.id,
        &as_template,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let from_template = courses::copy(
        &pool,
        a.scope(),
        template.id,
        &copy_to(a.tower, "S2"),
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();

    assert_eq!(
        (template.title.as_str(), template.template),
        ("S1 template", true)
    );
    assert_eq!(template.status, Status::Draft);
    assert_eq!(
        (from_template.title.as_str(), from_template.template),
        ("S1 template", false)
    );
    let published = courses::set_status(
        &pool,
        a.scope(),
        template.id,
        Status::Published,
        later_time(),
        Actor::Instance,
    )
    .await;
    assert!(matches!(published, Err(Error::Template)), "{published:?}");
    let entry = entries(&pool, a)
        .await
        .into_iter()
        .find(|e| e.action == "course.copy" && e.target_id == template.id)
        .expect("the template's entry");
    assert_eq!(entry.data["after"]["template"], true);
}

/// A refused copy inserts nothing and writes no entry: an unknown or other organization's
/// source or account, a taken code, an invalid title or code.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_refused_copy_writes_nothing(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap();
    let theirs = create(&pool, b, b.tower, "B1").await.unwrap();
    let before = (rows(&pool, a).await, entries(&pool, a).await.len());
    let before_b = rows(&pool, b).await;
    let copy = |source: Uuid, copy: CourseCopy| {
        let pool = &pool;
        async move {
            courses::copy(
                pool,
                a.scope(),
                source,
                &copy,
                later_time(),
                Actor::Instance,
            )
            .await
        }
    };

    let cases = [
        (copy(theirs.id, copy_to(a.tower, "X1")).await, "not found"),
        (copy(Uuid::nil(), copy_to(a.tower, "X2")).await, "not found"),
        (copy(source.id, copy_to(b.tower, "X3")).await, "account"),
        (copy(source.id, copy_to(Uuid::nil(), "X4")).await, "account"),
        (copy(source.id, copy_to(a.tower, "s1")).await, "taken"),
        (copy(source.id, copy_to(a.tower, " ")).await, "invalid"),
        (
            copy(
                source.id,
                CourseCopy {
                    title: Some(" ".into()),
                    ..copy_to(a.tower, "X5")
                },
            )
            .await,
            "invalid",
        ),
    ];
    for (result, expected) in cases {
        let got = match &result {
            Err(Error::NotFound) => "not found",
            Err(Error::AccountNotFound) => "account",
            Err(Error::CodeTaken) => "taken",
            Err(Error::CodeEmpty | Error::TitleEmpty) => "invalid",
            _ => "other",
        };
        assert_eq!(got, expected, "{result:?}");
    }
    assert_eq!(
        (rows(&pool, a).await, entries(&pool, a).await.len()),
        before
    );
    assert_eq!(rows(&pool, b).await, before_b);
}

/// A module titled `title` in `course`, published when `published`.
async fn module(pool: &PgPool, org: Org, course: Uuid, title: &str, published: bool) -> Uuid {
    let id = modules::create(pool, org.scope(), course, title, request_time())
        .await
        .expect("create module")
        .id;
    if published {
        let patch = ModulePatch {
            published: Some(true),
            ..ModulePatch::default()
        };
        modules::update(pool, org.scope(), id, &patch, request_time())
            .await
            .expect("publish module");
    }
    id
}

/// An item of `kind` titled `title` last in `module`, published when `published`.
async fn item(pool: &PgPool, org: Org, module: Uuid, kind: Kind, title: &str, published: bool) {
    let new = NewItem {
        kind,
        title: title.into(),
        ref_id: None,
        url: (kind == Kind::ExternalLink).then(|| "https://example.org/sop".into()),
        position: None,
    };
    let id = items::create(pool, org.scope(), module, &new, request_time())
        .await
        .expect("create item")
        .id;
    if published {
        let patch = items::ItemPatch {
            published: Some(true),
            ..items::ItemPatch::default()
        };
        items::update(pool, org.scope(), id, &patch, request_time())
            .await
            .expect("publish item");
    }
}

/// What a copy must keep of a module: title, position, published.
fn module_shape(m: &Module) -> (String, i32, bool) {
    (m.title.clone(), m.position, m.published)
}

/// What a copy must keep of an item, with its module's position in place of the module's id.
fn item_shape(
    i: &Item,
    modules: &[Module],
) -> (i32, i32, &'static str, String, bool, Option<String>) {
    let module = modules
        .iter()
        .find(|m| m.id == i.module_id)
        .expect("the item's module");
    (
        module.position,
        i.position,
        i.kind.as_str(),
        i.title.clone(),
        i.published,
        i.url.clone(),
    )
}

async fn outline(pool: &PgPool, org: Org, course: Uuid) -> (Vec<Module>, Vec<Item>) {
    let modules = modules::list_for_course(pool, org.scope(), course)
        .await
        .expect("read the outline");
    let items = items::list_for_course(pool, org.scope(), course)
        .await
        .expect("read the outline");
    (modules, items)
}

/// B13 with spec 03: a copy holds every module and item of the source with new ids, in the
/// same places, with the same titles, kinds, URLs and published flags; the source keeps its own.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_copy_copies_the_outline_with_positions_and_published_flags(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap().id;
    let first = module(&pool, a, source, "Ground", true).await;
    let second = module(&pool, a, source, "Local", false).await;
    item(&pool, a, first, Kind::Header, "Reading", true).await;
    item(&pool, a, first, Kind::ExternalLink, "SOP", false).await;
    item(&pool, a, first, Kind::Header, "Practice", true).await;
    item(&pool, a, second, Kind::ExternalLink, "Charts", true).await;
    let before = outline(&pool, a, source).await;

    let copy = courses::copy(
        &pool,
        a.scope(),
        source,
        &copy_to(a.enroute, "S1-B"),
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();

    let (modules, items) = outline(&pool, a, copy.id).await;
    let (source_modules, source_items) = outline(&pool, a, source).await;
    assert_eq!(
        (source_modules.clone(), source_items.clone()),
        before,
        "the source is unchanged"
    );
    assert_eq!(
        modules.iter().map(module_shape).collect::<Vec<_>>(),
        source_modules.iter().map(module_shape).collect::<Vec<_>>()
    );
    let shapes = |items: &[Item], modules: &[Module]| {
        let mut shapes: Vec<_> = items.iter().map(|i| item_shape(i, modules)).collect();
        shapes.sort();
        shapes
    };
    assert_eq!(
        shapes(&items, &modules),
        shapes(&source_items, &source_modules)
    );
    assert_eq!(items.len(), 4);
    for m in &modules {
        assert!(
            !source_modules.iter().any(|s| s.id == m.id),
            "a new module id"
        );
        assert_eq!((m.course_id, m.created_at), (copy.id, later_time()));
    }
    for i in &items {
        assert!(!source_items.iter().any(|s| s.id == i.id), "a new item id");
        assert_eq!(i.course_id, copy.id);
        assert!(
            modules.iter().any(|m| m.id == i.module_id),
            "in the copy's module"
        );
    }
}

/// Spec 04 with B13: a copy keeps each module's unlock and completion rules and each item's
/// requirement, and its prerequisites name the copies of their modules, never the source's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_copy_carries_the_rules_with_prerequisites_remapped(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap().id;
    let ground = module(&pool, a, source, "Ground", true).await;
    let local = module(&pool, a, source, "Local", true).await;
    let tower = module(&pool, a, source, "Tower", false).await;
    item(&pool, a, ground, Kind::ExternalLink, "SOP", true).await;
    item(&pool, a, ground, Kind::Header, "Practice", true).await;
    let link = items::list_for_module(&pool, a.scope(), ground)
        .await
        .unwrap()[0]
        .id;
    items::set_requirement(
        &pool,
        a.scope(),
        link,
        Some(Requirement::MarkDone),
        request_time(),
    )
    .await
    .unwrap();
    let unlock_at = UNIX_EPOCH + Duration::from_micros(1_900_000_000_123_456);
    let rules = |prerequisites: &[Uuid]| RulesPatch {
        unlock_at: Some(Some(unlock_at)),
        prerequisite_ids: Some(prerequisites.to_vec()),
        require_sequential: Some(true),
        completion_rule: Some(CompletionRule::One),
    };
    modules::set_rules(&pool, a.scope(), local, &rules(&[ground]), request_time())
        .await
        .unwrap();
    modules::set_rules(
        &pool,
        a.scope(),
        tower,
        &rules(&[ground, local]),
        request_time(),
    )
    .await
    .unwrap();
    let before = outline(&pool, a, source).await;

    let copy = courses::copy(
        &pool,
        a.scope(),
        source,
        &copy_to(a.enroute, "S1-B"),
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();

    assert_eq!(
        outline(&pool, a, source).await,
        before,
        "the source is unchanged"
    );
    let (modules, items) = outline(&pool, a, copy.id).await;
    let ids: Vec<Uuid> = modules.iter().map(|m| m.id).collect();
    let rule_shape = |m: &Module| (m.unlock_at, m.require_sequential, m.completion_rule);
    assert_eq!(rule_shape(&modules[0]), (None, false, CompletionRule::All));
    for m in &modules[1..] {
        assert_eq!(rule_shape(m), (Some(unlock_at), true, CompletionRule::One));
    }
    assert!(modules[0].prerequisite_ids.is_empty());
    assert_eq!(modules[1].prerequisite_ids, [ids[0]]);
    assert_eq!(modules[2].prerequisite_ids, [ids[0], ids[1]]);
    let copied_link = items
        .iter()
        .find(|i| i.kind == Kind::ExternalLink)
        .expect("the copied link");
    assert_eq!(copied_link.requirement, Some(Requirement::MarkDone));
    assert!(
        items
            .iter()
            .filter(|i| i.kind == Kind::Header)
            .all(|i| i.requirement.is_none())
    );
    let rows: Vec<(Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT course_id, module_id, requires_id FROM module_prerequisites
         WHERE org_id = $1 AND course_id = $2 ORDER BY module_id, requires_id",
    )
    .bind(a.id)
    .bind(copy.id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 3);
    for (course, module, requires) in rows {
        assert_eq!(course, copy.id);
        assert!(
            ids.contains(&module) && ids.contains(&requires),
            "a copied module"
        );
    }
}

/// An item whose kind this release cannot copy (its content spec is not built) refuses the
/// copy, and nothing is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_source_with_content_no_copy_can_take_is_refused(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap().id;
    let first = module(&pool, a, source, "Ground", true).await;
    item(&pool, a, first, Kind::Header, "Reading", true).await;
    sqlx::query(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, ref_id, title)
         VALUES (gen_random_uuid(), $1, $2, $3, 1, 'page', gen_random_uuid(), 'A page')",
    )
    .bind(a.id)
    .bind(source)
    .bind(first)
    .execute(&pool)
    .await
    .unwrap();
    let before = (rows(&pool, a).await, entries(&pool, a).await.len());

    let refused = courses::copy(
        &pool,
        a.scope(),
        source,
        &copy_to(a.tower, "S1-B"),
        later_time(),
        Actor::Instance,
    )
    .await;

    assert!(
        matches!(&refused, Err(Error::ContentNotCopyable { kind, .. }) if kind == "page"),
        "{refused:?}"
    );
    assert_eq!(
        (rows(&pool, a).await, entries(&pool, a).await.len()),
        before
    );
    let modules: i64 = sqlx::query_scalar("SELECT count(*) FROM modules WHERE org_id = $1")
        .bind(a.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(modules, 1, "no module was copied");
}

/// A stored URL `items::url_is_valid` refuses (which the table's check lets through) refuses the
/// copy, and nothing is written.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_source_with_an_invalid_url_is_refused(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap().id;
    let first = module(&pool, a, source, "Ground", true).await;
    let bad = Uuid::from_u128(7);
    sqlx::query(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, url, title)
         VALUES ($4, $1, $2, $3, 0, 'external_link', 'https://', 'No host')",
    )
    .bind(a.id)
    .bind(source)
    .bind(first)
    .bind(bad)
    .execute(&pool)
    .await
    .unwrap();
    let before = (rows(&pool, a).await, entries(&pool, a).await.len());

    let refused = courses::copy(
        &pool,
        a.scope(),
        source,
        &copy_to(a.tower, "S1-B"),
        later_time(),
        Actor::Instance,
    )
    .await;

    assert!(
        matches!(refused, Err(Error::CopiedUrlInvalid { item_id }) if item_id == bad),
        "{refused:?}"
    );
    assert_eq!(
        (rows(&pool, a).await, entries(&pool, a).await.len()),
        before
    );
}

/// The copy reads the outline under the source's course row, which every outline write locks
/// first (spec 03 B12): a write in progress is waited for and its module is copied.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_copy_waits_for_an_outline_write_in_progress(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = create(&pool, a, a.tower, "S1").await.unwrap().id;
    module(&pool, a, source, "Ground", true).await;
    let mut writer = pool.begin().await.unwrap();
    modules::create(&mut *writer, a.scope(), source, "Local", request_time())
        .await
        .unwrap();

    let copying = {
        let pool = pool.clone();
        tokio::spawn(async move {
            courses::copy(
                &pool,
                a.scope(),
                source,
                &copy_to(a.tower, "S1-B"),
                later_time(),
                Actor::Instance,
            )
            .await
        })
    };
    for _ in 0..500 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    writer.commit().await.unwrap();

    let copy = tokio::time::timeout(Duration::from_secs(10), copying)
        .await
        .expect("the copy finishes once the write commits")
        .expect("the task does not panic")
        .unwrap();
    let titles: Vec<String> = modules::list_for_course(&pool, a.scope(), copy.id)
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.title)
        .collect();
    assert_eq!(titles, ["Ground", "Local"]);
}
