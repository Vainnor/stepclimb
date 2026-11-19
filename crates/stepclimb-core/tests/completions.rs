// SPDX-License-Identifier: AGPL-3.0-or-later
//! Completion history and validity against a fresh database per test
//! (docs/specs/m1/14-recurrence-and-expiring-completions.md B1 to B4, B13; T1, T9 at the
//! repository). The routes and their permission cases are in the server's `tests/completions.rs`
//! and `tests/permissions.rs`. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, OrgSettingsPatch, Source,
    audit::Actor,
    completions::{self, CourseFilter, TargetType},
    courses::{self, CourseCopy, CoursePatch, NewCourse},
    enrollments::{self, NewEnrollment},
    id::{Uuid, new_id},
    items::{self, ItemPatch, Kind, NewItem},
    modules::{self, ModulePatch},
    organizations,
    progress::{self, Action},
    roles,
    rules::Requirement,
    users::{self, NewUser},
};

/// The time a parse of `rfc3339` names.
fn at(rfc3339: &str) -> SystemTime {
    courses::parse_timestamp(rfc3339).expect("an RFC 3339 timestamp")
}

/// 2025-11-13T02:13:20Z: when the content is written.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the credential resolvers"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An organization in the time zone it names, with one course in its root account.
#[derive(Clone, Copy)]
struct Org {
    id: Uuid,
    root: Uuid,
    course: Uuid,
}

impl Org {
    async fn new(pool: &PgPool, slug: &str, tz: &str) -> Self {
        roles::sync_builtin(pool, t0()).await.expect("sync roles");
        let id = organizations::create(pool, slug, slug, t0())
            .await
            .expect("create organization")
            .id;
        let patch = OrgSettingsPatch {
            default_tz: Some(tz.into()),
            ..OrgSettingsPatch::default()
        };
        organizations::update_settings(pool, scope(id), &patch, t0(), Actor::Instance)
            .await
            .expect("set the time zone");
        let root: Uuid = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
            .expect("the root");
        let mut org = Self {
            id,
            root,
            course: Uuid::nil(),
        };
        org.course = org.new_course(pool, "Course").await;
        org
    }

    fn scope(self) -> OrgScope {
        scope(self.id)
    }

    async fn new_course(self, pool: &PgPool, title: &str) -> Uuid {
        let hex = new_id().simple().to_string();
        let new = NewCourse {
            account_id: self.root,
            title: title.into(),
            code: format!("C{}", &hex[hex.len() - 12..]),
        };
        courses::create(pool, self.scope(), &new, t0(), Actor::Instance)
            .await
            .expect("create course")
            .id
    }

    async fn course_validity(self, pool: &PgPool, months: Option<i16>) {
        let patch = CoursePatch {
            completion_valid_months: Some(months),
            ..CoursePatch::default()
        };
        courses::update(
            pool,
            self.scope(),
            self.course,
            &patch,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("set the course's validity");
    }

    /// A published module, last in the course, titled `title`, valid `months`.
    async fn module(self, pool: &PgPool, title: &str, months: Option<i16>) -> Uuid {
        let id = modules::create(pool, self.scope(), self.course, title, t0())
            .await
            .expect("create module")
            .id;
        let patch = ModulePatch {
            published: Some(true),
            completion_valid_months: Some(months),
            ..ModulePatch::default()
        };
        modules::update(pool, self.scope(), id, &patch, t0())
            .await
            .expect("publish module");
        id
    }

    /// A published external link in `module` that a view completes.
    async fn item(self, pool: &PgPool, module: Uuid) -> Uuid {
        let new = NewItem {
            kind: Kind::ExternalLink,
            title: "Link".into(),
            ref_id: None,
            url: Some("https://example.com/".into()),
            position: None,
        };
        let id = items::create(pool, self.scope(), module, &new, t0())
            .await
            .expect("create item")
            .id;
        let patch = ItemPatch {
            published: Some(true),
            ..ItemPatch::default()
        };
        items::update(pool, self.scope(), id, &patch, t0())
            .await
            .expect("publish item");
        let view = Some(Requirement::View);
        items::set_requirement(pool, self.scope(), id, view, t0())
            .await
            .expect("set requirement");
        id
    }

    /// A new Student of the course named `name`.
    async fn student(self, pool: &PgPool, name: &str) -> Uuid {
        let user = NewUser {
            display_name: name.into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!("{}@example.test", new_id().simple())),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let user = users::create(pool, self.scope(), &user, t0())
            .await
            .expect("create user")
            .id;
        let role_id = roles::builtin_id(pool, "student")
            .await
            .expect("read role")
            .expect("the Student role");
        let new = NewEnrollment {
            user_id: user,
            role_id,
            source: Source::Local,
            external_id: None,
            observed_user_ids: Vec::new(),
        };
        enrollments::create(pool, self.scope(), self.course, &new, t0(), Actor::Instance)
            .await
            .expect("enroll");
        user
    }

    async fn view(self, pool: &PgPool, user: Uuid, item: Uuid, now: SystemTime) {
        let mut tx = pool.begin().await.expect("begin");
        let recorded = progress::record_step(&mut tx, self.scope(), user, item, Action::View, now)
            .await
            .expect("record the step");
        assert!(!recorded.events.is_empty(), "the view completes the item");
        tx.commit().await.expect("commit");
    }

    async fn history(self, pool: &PgPool, filter: &CourseFilter) -> Vec<completions::Completion> {
        completions::list_for_course(pool, self.scope(), self.course, filter, None, 100)
            .await
            .expect("list completions")
    }
}

/// The `(target type, target, cycle, expires_at)` of each completion, newest first.
type Summary = (TargetType, Uuid, i32, Option<SystemTime>);

fn summary(rows: &[completions::Completion]) -> Vec<Summary> {
    rows.iter()
        .map(|c| (c.target_type, c.target_id, c.cycle, c.expires_at))
        .collect()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_a_module_and_its_course_record_one_completion_each_with_their_expiry(pool: PgPool) {
    // 03:00 UTC on 31 January is 22:00 on 30 January in New York, where the months are counted.
    let org = Org::new(&pool, "a", "America/New_York").await;
    org.course_validity(&pool, Some(12)).await;
    let module = org.module(&pool, "Module", Some(6)).await;
    let item = org.item(&pool, module).await;
    let user = org.student(&pool, "Learner").await;
    let done = at("2026-01-31T03:00:00Z");
    org.view(&pool, user, item, done).await;
    let rows = org.history(&pool, &CourseFilter::default()).await;
    let six_months = Some(at("2026-07-31T02:00:00Z"));
    let twelve_months = Some(at("2027-01-31T03:00:00Z"));
    let mut found = summary(&rows);
    found.sort_by_key(|row| row.0 == TargetType::Module);
    assert_eq!(
        found,
        [
            (TargetType::Course, org.course, 1, twelve_months),
            (TargetType::Module, module, 1, six_months),
        ]
    );
    assert!(rows.iter().all(|c| c.completed_at == done
        && c.expired_at.is_none()
        && c.user_id == user
        && c.display_name == "Learner"));
    let module_row = rows.iter().find(|c| c.target_type == TargetType::Module);
    assert_eq!(
        module_row.and_then(|c| c.target_title.as_deref()),
        Some("Module")
    );

    let mut conn = pool.acquire().await.expect("a connection");
    let mine = progress::for_learner_in_course(&mut conn, org.scope(), user, org.course)
        .await
        .expect("read progress");
    assert_eq!(mine.course.map(|c| c.expires_at), Some(twelve_months));
    assert_eq!(mine.modules[0].expires_at, six_months);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_31_january_plus_one_month_is_the_last_day_of_february(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    let module = org.module(&pool, "Module", Some(1)).await;
    let item = org.item(&pool, module).await;
    let user = org.student(&pool, "Learner").await;
    org.view(&pool, user, item, at("2026-01-31T12:00:00Z"))
        .await;
    let filter = CourseFilter {
        target_type: Some(TargetType::Module),
        ..CourseFilter::default()
    };
    let rows = org.history(&pool, &filter).await;
    assert_eq!(
        summary(&rows),
        [(
            TargetType::Module,
            module,
            1,
            Some(at("2026-02-28T12:00:00Z"))
        )]
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b4_no_validity_records_no_expiry_and_items_record_nothing(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    let first = org.module(&pool, "First", None).await;
    let second = org.module(&pool, "Second", None).await;
    let item = org.item(&pool, first).await;
    org.item(&pool, second).await;
    let user = org.student(&pool, "Learner").await;
    org.view(&pool, user, item, t0()).await;
    let rows = org.history(&pool, &CourseFilter::default()).await;
    assert_eq!(
        summary(&rows),
        [(TargetType::Module, first, 1, None)],
        "the item records nothing and the course is not complete"
    );
    let mut conn = pool.acquire().await.expect("a connection");
    let mine = progress::for_learner_in_course(&mut conn, org.scope(), user, org.course)
        .await
        .expect("read progress");
    assert_eq!(mine.modules[0].expires_at, None);
    let course = mine.course.expect("a course row");
    assert_eq!((course.completed_at, course.expires_at), (None, None));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t9_a_validity_change_applies_to_later_completions_only(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    org.course_validity(&pool, Some(12)).await;
    let module = org.module(&pool, "Module", None).await;
    let item = org.item(&pool, module).await;
    let (early, late) = (
        org.student(&pool, "Early").await,
        org.student(&pool, "Late").await,
    );
    let done = at("2026-03-10T00:00:00Z");
    org.view(&pool, early, item, done).await;
    org.course_validity(&pool, Some(24)).await;
    org.view(&pool, late, item, done).await;
    let rows = org.history(&pool, &CourseFilter::default()).await;
    let expiry = |rows: &[completions::Completion], user: Uuid| {
        rows.iter()
            .find(|c| c.user_id == user && c.target_type == TargetType::Course)
            .and_then(|c| c.expires_at)
    };
    assert_eq!(
        expiry(&rows, early),
        Some(at("2027-03-10T00:00:00Z")),
        "unchanged"
    );
    assert_eq!(expiry(&rows, late), Some(at("2028-03-10T00:00:00Z")));
    org.course_validity(&pool, None).await;
    let rows = org.history(&pool, &CourseFilter::default()).await;
    assert_eq!(
        expiry(&rows, early),
        Some(at("2027-03-10T00:00:00Z")),
        "clearing the validity leaves stored expiry dates"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b1_a_completion_after_a_reset_is_the_next_cycle(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    let module = org.module(&pool, "Module", Some(12)).await;
    let item = org.item(&pool, module).await;
    let user = org.student(&pool, "Learner").await;
    org.view(&pool, user, item, at("2026-03-10T00:00:00Z"))
        .await;
    // What spec 14 I2's reset deletes; the completions stay.
    for table in ["item_progress", "module_progress", "course_progress"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE user_id = $1"
        )))
        .bind(user)
        .execute(&pool)
        .await
        .expect("reset");
    }
    org.view(&pool, user, item, at("2027-03-01T00:00:00Z"))
        .await;
    let filter = CourseFilter {
        target_type: Some(TargetType::Module),
        ..CourseFilter::default()
    };
    assert_eq!(
        summary(&org.history(&pool, &filter).await),
        [
            (
                TargetType::Module,
                module,
                2,
                Some(at("2028-03-01T00:00:00Z"))
            ),
            (
                TargetType::Module,
                module,
                1,
                Some(at("2027-03-10T00:00:00Z"))
            ),
        ],
        "newest first"
    );
    let mut conn = pool.acquire().await.expect("a connection");
    let mine = progress::for_learner_in_course(&mut conn, org.scope(), user, org.course)
        .await
        .expect("read progress");
    assert_eq!(
        mine.modules[0].expires_at,
        Some(at("2028-03-01T00:00:00Z")),
        "the current completion is the newest cycle"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b13_lists_filter_by_user_type_observed_learners_and_published_modules(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    let shown = org.module(&pool, "Shown", None).await;
    let hidden = org.module(&pool, "Hidden", None).await;
    let (shown_item, hidden_item) = (org.item(&pool, shown).await, org.item(&pool, hidden).await);
    let (ann, bob) = (
        org.student(&pool, "Ann").await,
        org.student(&pool, "Bob").await,
    );
    for (user, minute) in [(ann, 0), (bob, 1)] {
        let base = at("2026-03-10T00:00:00Z") + Duration::from_secs(minute * 60);
        org.view(&pool, user, shown_item, base).await;
        org.view(&pool, user, hidden_item, base + Duration::from_secs(10))
            .await;
    }
    let unpublish = ModulePatch {
        published: Some(false),
        ..ModulePatch::default()
    };
    modules::update(&pool, org.scope(), hidden, &unpublish, t0())
        .await
        .expect("unpublish");
    let all = org.history(&pool, &CourseFilter::default()).await;
    assert_eq!(all.len(), 6, "two modules and the course for each");
    let times: Vec<SystemTime> = all.iter().map(|c| c.completed_at).collect();
    assert!(times.windows(2).all(|w| w[0] >= w[1]), "newest first");

    let for_ann = CourseFilter {
        user_id: Some(ann),
        ..CourseFilter::default()
    };
    assert!(
        org.history(&pool, &for_ann)
            .await
            .iter()
            .all(|c| c.user_id == ann)
    );
    assert_eq!(org.history(&pool, &for_ann).await.len(), 3);
    let observed = CourseFilter {
        only: Some(vec![bob]),
        ..CourseFilter::default()
    };
    let rows = org.history(&pool, &observed).await;
    assert!(rows.len() == 3 && rows.iter().all(|c| c.user_id == bob));
    let nobody = CourseFilter {
        only: Some(Vec::new()),
        ..CourseFilter::default()
    };
    assert!(org.history(&pool, &nobody).await.is_empty());
    let courses_only = CourseFilter {
        target_type: Some(TargetType::Course),
        ..CourseFilter::default()
    };
    let rows = org.history(&pool, &courses_only).await;
    assert!(rows.len() == 2 && rows.iter().all(|c| c.target_type == TargetType::Course));
    let published = CourseFilter {
        published_only: true,
        ..CourseFilter::default()
    };
    let rows = org.history(&pool, &published).await;
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|c| c.target_id != hidden), "{rows:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b13_pages_follow_the_cursor_and_lists_stay_in_the_organization(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    let other = Org::new(&pool, "b", "UTC").await;
    let mut items = Vec::new();
    for title in ["One", "Two", "Three"] {
        let module = org.module(&pool, title, None).await;
        items.push(org.item(&pool, module).await);
    }
    let user = org.student(&pool, "Learner").await;
    for (minute, item) in (0u64..).zip(&items) {
        org.view(&pool, user, *item, t0() + Duration::from_secs(minute * 60))
            .await;
    }
    let other_module = other.module(&pool, "Elsewhere", None).await;
    let other_item = other.item(&pool, other_module).await;
    let other_user = other.student(&pool, "Elsewhere").await;
    other.view(&pool, other_user, other_item, t0()).await;

    let mut seen = Vec::new();
    let mut after: Option<completions::ListKey> = None;
    loop {
        let page = completions::list_for_user(&pool, org.scope(), user, None, after.as_ref(), 2)
            .await
            .expect("a page");
        let Some(last) = page.last() else { break };
        let micros = stepclimb_core::db::epoch_micros(last.completed_at).expect("a time");
        after = Some((micros, last.id));
        seen.extend(page.into_iter().map(|c| c.id));
    }
    assert_eq!(seen.len(), 4, "three modules and the course, each once");
    let mut unique = seen.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 4);

    let in_course =
        completions::list_for_user(&pool, org.scope(), user, Some(other.course), None, 10)
            .await
            .expect("list");
    assert!(
        in_course.is_empty(),
        "another organization's course id names nothing"
    );
    let elsewhere = completions::list_for_user(&pool, org.scope(), other_user, None, None, 10)
        .await
        .expect("list");
    assert!(
        elsewhere.is_empty(),
        "another organization's user names nothing"
    );
    let other_course = completions::list_for_course(
        &pool,
        org.scope(),
        other.course,
        &CourseFilter::default(),
        None,
        10,
    )
    .await
    .expect("list");
    assert!(other_course.is_empty());
    let before_epoch =
        completions::list_for_user(&pool, org.scope(), user, None, Some(&(-1, Uuid::nil())), 10)
            .await;
    assert!(matches!(
        before_epoch,
        Err(completions::Error::InvalidCursor)
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_copy_takes_the_course_and_module_validity(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    org.course_validity(&pool, Some(12)).await;
    org.module(&pool, "Module", Some(6)).await;
    let copy = CourseCopy {
        account_id: org.root,
        title: None,
        code: "COPY1".into(),
        template: false,
    };
    let copied = courses::copy(&pool, org.scope(), org.course, &copy, t0(), Actor::Instance)
        .await
        .expect("copy");
    assert_eq!(copied.completion_valid_months, Some(12));
    let modules = modules::list_for_course(&pool, org.scope(), copied.id)
        .await
        .expect("list modules");
    assert_eq!(modules[0].completion_valid_months, Some(6));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b3_a_validity_outside_1_to_120_months_is_refused(pool: PgPool) {
    let org = Org::new(&pool, "a", "UTC").await;
    let module = org.module(&pool, "Module", None).await;
    for months in [0, 121, -3] {
        let patch = CoursePatch {
            completion_valid_months: Some(Some(months)),
            ..CoursePatch::default()
        };
        let refused = courses::update(
            &pool,
            org.scope(),
            org.course,
            &patch,
            t0(),
            Actor::Instance,
        )
        .await;
        assert!(
            matches!(refused, Err(courses::Error::ValidityOutOfRange)),
            "{months}: {refused:?}"
        );
        let patch = ModulePatch {
            completion_valid_months: Some(Some(months)),
            ..ModulePatch::default()
        };
        let refused = modules::update(&pool, org.scope(), module, &patch, t0()).await;
        assert!(
            matches!(refused, Err(modules::Error::ValidityOutOfRange)),
            "{months}: {refused:?}"
        );
    }
    let stored: (Option<i16>, Option<i16>) = sqlx::query_as(
        "SELECT c.completion_valid_months, m.completion_valid_months
         FROM courses c JOIN modules m ON m.course_id = c.id WHERE c.id = $1",
    )
    .bind(org.course)
    .fetch_one(&pool)
    .await
    .expect("read");
    assert_eq!(stored, (None, None));
}
