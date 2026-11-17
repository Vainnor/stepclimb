// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recording learner progress and rolling it up, against a fresh database per test
//! (docs/specs/m1/05-progress-and-events.md B1 to B7, B10, B12, B13, B16; T2 to T7, T9, T13 at
//! the repository). The route, its outbox rows and the permission cases are in the server's
//! `tests/progress.rs` and `tests/permissions.rs`. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    courses::{self, NewCourse},
    enrollments::{self, NewEnrollment},
    id::Uuid,
    items::{self, ItemPatch, Kind, NewItem},
    modules::{self, ModulePatch, RulesPatch},
    organizations,
    progress::{
        self, Action, Completion, CourseCompleted, Error, Event, ItemCompleted, ModuleCompleted,
        Recorded, State,
    },
    roles,
    rules::{CompletionRule, Requirement, RequirementKind},
    users::{self, NewUser},
};

/// 2025-11-13T02:13:20Z.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn later(seconds: u64) -> SystemTime {
    t0() + Duration::from_secs(seconds)
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
        roles::sync_builtin(pool, t0()).await.expect("sync roles");
        let id = organizations::create(pool, slug, slug, t0())
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
        let course = courses::create(pool, scope(id), &new, t0(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        Self { id, course }
    }

    fn scope(self) -> OrgScope {
        scope(self.id)
    }

    /// A published module, last in the course.
    async fn module(self, pool: &PgPool) -> Uuid {
        let id = modules::create(pool, self.scope(), self.course, "Module", t0())
            .await
            .expect("create module")
            .id;
        let patch = ModulePatch {
            published: Some(true),
            ..ModulePatch::default()
        };
        modules::update(pool, self.scope(), id, &patch, t0())
            .await
            .expect("publish module");
        id
    }

    /// A published external link in `module`, last, with `requirement`.
    async fn item(self, pool: &PgPool, module: Uuid, requirement: Option<Requirement>) -> Uuid {
        let new = NewItem {
            kind: Kind::ExternalLink,
            title: "Link title".into(),
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
        items::set_requirement(pool, self.scope(), id, requirement, t0())
            .await
            .expect("set requirement");
        id
    }

    /// A published header in `module`, last.
    async fn header(self, pool: &PgPool, module: Uuid) -> Uuid {
        let new = NewItem {
            kind: Kind::Header,
            title: "Header".into(),
            ref_id: None,
            url: None,
            position: None,
        };
        let id = items::create(pool, self.scope(), module, &new, t0())
            .await
            .expect("create header")
            .id;
        let patch = ItemPatch {
            published: Some(true),
            ..ItemPatch::default()
        };
        items::update(pool, self.scope(), id, &patch, t0())
            .await
            .expect("publish header");
        id
    }

    /// A new user enrolled in the course with the built-in role `role`: the user and the
    /// enrollment.
    async fn enrolled(self, pool: &PgPool, role: &str) -> (Uuid, Uuid) {
        let user = NewUser {
            display_name: "Learner Name".into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!(
                "{}@example.test",
                stepclimb_core::id::new_id().simple()
            )),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        let user = users::create(pool, self.scope(), &user, t0())
            .await
            .expect("create user")
            .id;
        let role_id = roles::builtin_id(pool, role)
            .await
            .expect("read role")
            .expect("a built-in role");
        let new = NewEnrollment {
            user_id: user,
            role_id,
            source: Source::Local,
            external_id: None,
        };
        let enrollment =
            enrollments::create(pool, self.scope(), self.course, &new, t0(), Actor::Instance)
                .await
                .expect("enroll")
                .id;
        (user, enrollment)
    }

    async fn student(self, pool: &PgPool) -> Uuid {
        self.enrolled(pool, "student").await.0
    }

    async fn rules(self, pool: &PgPool, module: Uuid, patch: RulesPatch) {
        modules::set_rules(pool, self.scope(), module, &patch, t0())
            .await
            .expect("set rules");
    }

    async fn step(
        self,
        pool: &PgPool,
        user: Uuid,
        item: Uuid,
        action: Action,
        now: SystemTime,
    ) -> Result<Recorded, Error> {
        let mut tx = pool.begin().await.expect("begin");
        let recorded = progress::record_step(&mut tx, self.scope(), user, item, action, now).await;
        if recorded.is_ok() {
            tx.commit().await.expect("commit");
        }
        recorded
    }
}

async fn rows(pool: &PgPool, table: &str, user: Uuid) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM {table} WHERE user_id = $1"
    )))
    .bind(user)
    .fetch_one(pool)
    .await
    .expect("count rows")
}

async fn item_updated_at(pool: &PgPool, user: Uuid, item: Uuid) -> SystemTime {
    let micros: i64 = sqlx::query_scalar(
        "SELECT (extract(epoch FROM updated_at) * 1000000)::bigint
         FROM item_progress WHERE user_id = $1 AND item_id = $2",
    )
    .bind(user)
    .bind(item)
    .fetch_one(pool)
    .await
    .expect("the row");
    UNIX_EPOCH + Duration::from_micros(u64::try_from(micros).expect("after 1970"))
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_view_completes_a_view_requirement(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::View)).await;
    let user = org.student(&pool).await;
    let recorded = org
        .step(&pool, user, item, Action::View, t0())
        .await
        .unwrap();
    assert_eq!(recorded.progress.state, State::Completed);
    assert_eq!(recorded.progress.started_at, t0());
    assert_eq!(recorded.progress.completed_at, Some(t0()));
    assert!(matches!(
        recorded.events.first(),
        Some(Event::ItemCompleted(ItemCompleted {
            requirement_kind: RequirementKind::View,
            item_kind: Kind::ExternalLink,
            ..
        }))
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_view_starts_a_mark_done_item_and_mark_done_completes_it(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::MarkDone)).await;
    let user = org.student(&pool).await;
    let started = org
        .step(&pool, user, item, Action::View, t0())
        .await
        .unwrap();
    assert_eq!(started.progress.state, State::Started);
    assert_eq!(started.progress.completed_at, None);
    assert!(started.events.is_empty(), "T9: a start writes no event");
    let done = org
        .step(&pool, user, item, Action::MarkDone, later(60))
        .await
        .unwrap();
    assert_eq!(done.progress.state, State::Completed);
    assert_eq!(
        done.progress.started_at,
        t0(),
        "the first step's time stays"
    );
    assert_eq!(done.progress.completed_at, Some(later(60)));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_step_the_item_does_not_take_is_refused(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let view = org.item(&pool, module, Some(Requirement::View)).await;
    let plain = org.item(&pool, module, None).await;
    let header = org.header(&pool, module).await;
    let user = org.student(&pool).await;
    for (item, action) in [
        (view, Action::MarkDone),
        (plain, Action::MarkDone),
        (header, Action::View),
    ] {
        let refused = org.step(&pool, user, item, action, t0()).await;
        assert!(matches!(refused, Err(Error::ActionInvalid)), "{refused:?}");
    }
    assert_eq!(rows(&pool, "item_progress", user).await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_a_step_that_changes_nothing_writes_nothing(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let plain = org.item(&pool, module, None).await;
    let view = org.item(&pool, module, Some(Requirement::View)).await;
    let user = org.student(&pool).await;
    for item in [plain, view] {
        let first = org
            .step(&pool, user, item, Action::View, t0())
            .await
            .unwrap();
        let again = org
            .step(&pool, user, item, Action::View, later(30))
            .await
            .unwrap();
        assert_eq!(again.progress, first.progress);
        assert!(again.events.is_empty());
        assert_eq!(item_updated_at(&pool, user, item).await, t0());
    }
    let plain_row = org
        .step(&pool, user, plain, Action::View, t0())
        .await
        .unwrap();
    assert_eq!(
        plain_row.progress.state,
        State::Started,
        "B3: an item without a requirement never completes"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_a_locked_item_is_refused_and_nothing_is_written(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let first = org.module(&pool).await;
    let second = org.module(&pool).await;
    org.item(&pool, first, Some(Requirement::MarkDone)).await;
    let waiting = org.item(&pool, second, Some(Requirement::View)).await;
    org.rules(
        &pool,
        second,
        RulesPatch {
            prerequisite_ids: Some(vec![first]),
            ..RulesPatch::default()
        },
    )
    .await;
    let user = org.student(&pool).await;
    let refused = org.step(&pool, user, waiting, Action::View, t0()).await;
    let Err(Error::Locked(reasons)) = refused else {
        panic!("expected a lock, got {refused:?}");
    };
    assert!(!reasons.is_empty());
    for table in ["item_progress", "module_progress", "course_progress"] {
        assert_eq!(rows(&pool, table, user).await, 0, "{table}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_an_archived_course_takes_no_progress(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::View)).await;
    let user = org.student(&pool).await;
    for status in [courses::Status::Published, courses::Status::Archived] {
        courses::set_status(
            &pool,
            org.scope(),
            org.course,
            status,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("set status");
    }
    let refused = org.step(&pool, user, item, Action::View, t0()).await;
    assert!(matches!(refused, Err(Error::CourseArchived)), "{refused:?}");
    assert_eq!(rows(&pool, "item_progress", user).await, 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b1_only_an_active_participating_enrollment_records_progress(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::View)).await;
    let (designer, _) = org.enrolled(&pool, "course_designer").await;
    let refused = org.step(&pool, designer, item, Action::View, t0()).await;
    assert!(matches!(refused, Err(Error::NotLearner)), "{refused:?}");
    let (concluded, enrollment) = org.enrolled(&pool, "student").await;
    enrollments::set_status(
        &pool,
        org.scope(),
        enrollment,
        enrollments::Status::Concluded,
        t0(),
        Actor::Instance,
    )
    .await
    .expect("conclude");
    let refused = org.step(&pool, concluded, item, Action::View, t0()).await;
    assert!(matches!(refused, Err(Error::NotLearner)), "{refused:?}");
    for user in [designer, concluded] {
        assert_eq!(rows(&pool, "item_progress", user).await, 0);
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_unpublished_item_does_not_exist_for_the_learner(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::View)).await;
    let patch = ItemPatch {
        published: Some(false),
        ..ItemPatch::default()
    };
    items::update(&pool, org.scope(), item, &patch, t0())
        .await
        .expect("unpublish");
    let user = org.student(&pool).await;
    let refused = org.step(&pool, user, item, Action::View, t0()).await;
    assert!(matches!(refused, Err(Error::ItemNotFound)), "{refused:?}");
    let other = Org::new(&pool, "b").await;
    let refused = other.step(&pool, user, item, Action::View, t0()).await;
    assert!(
        matches!(refused, Err(Error::ItemNotFound)),
        "another organization's item: {refused:?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t4_a_completed_row_outlives_requirement_changes(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::View)).await;
    let user = org.student(&pool).await;
    let _ = org
        .step(&pool, user, item, Action::View, t0())
        .await
        .unwrap();
    for requirement in [None, Some(Requirement::MarkDone)] {
        items::set_requirement(&pool, org.scope(), item, requirement, later(10))
            .await
            .expect("change requirement");
    }
    let again = org
        .step(&pool, user, item, Action::MarkDone, later(20))
        .await
        .unwrap();
    assert_eq!(again.progress.state, State::Completed);
    assert_eq!(again.progress.completed_at, Some(t0()));
    assert!(again.events.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t5_the_last_requirement_completes_item_module_and_course_once(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let first = org.module(&pool).await;
    let second = org.module(&pool).await;
    let a = org.item(&pool, first, Some(Requirement::View)).await;
    let b = org.item(&pool, second, Some(Requirement::MarkDone)).await;
    org.item(&pool, second, None).await;
    let user = org.student(&pool).await;
    let one = org.step(&pool, user, a, Action::View, t0()).await.unwrap();
    assert_eq!(one.events.len(), 2, "item and module: {:?}", one.events);
    let last = org
        .step(&pool, user, b, Action::MarkDone, later(5))
        .await
        .unwrap();
    assert_eq!(
        last.events,
        [
            Event::ItemCompleted(ItemCompleted {
                user_id: user,
                course_id: org.course,
                module_id: second,
                item_id: b,
                item_kind: Kind::ExternalLink,
                requirement_kind: RequirementKind::MarkDone,
                completed_at: later(5),
            }),
            Event::ModuleCompleted(ModuleCompleted {
                user_id: user,
                course_id: org.course,
                module_id: second,
                completed_at: later(5),
            }),
            Event::CourseCompleted(CourseCompleted {
                user_id: user,
                course_id: org.course,
                completed_at: later(5),
            }),
        ]
    );
    let again = org
        .step(&pool, user, b, Action::MarkDone, later(9))
        .await
        .unwrap();
    assert!(again.events.is_empty());
    let mut conn = pool.acquire().await.unwrap();
    let stored = progress::for_learner_in_course(&mut conn, org.scope(), user, org.course)
        .await
        .unwrap();
    let course = stored.course.expect("the course row");
    assert_eq!(
        (
            course.required_count,
            course.completed_count,
            course.completed_at
        ),
        (2, 2, Some(later(5)))
    );
    let modules: Vec<_> = stored
        .modules
        .iter()
        .map(|m| {
            (
                m.module_id,
                m.required_count,
                m.completed_count,
                m.completed_at,
            )
        })
        .collect();
    assert_eq!(
        modules,
        [(first, 1, 1, Some(t0())), (second, 1, 1, Some(later(5)))]
    );
    let items: Vec<_> = stored.items.iter().map(|i| i.item_id).collect();
    assert_eq!(items, [a, b]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t6_concurrent_completions_take_turns(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let a = org.item(&pool, module, Some(Requirement::View)).await;
    let b = org.item(&pool, module, Some(Requirement::View)).await;
    let user = org.student(&pool).await;
    let mut first = pool.begin().await.unwrap();
    let one = progress::record_step(&mut first, org.scope(), user, a, Action::View, t0())
        .await
        .unwrap();
    // The second completion starts while the first holds the learner's course row.
    let second = tokio::spawn({
        let pool = pool.clone();
        async move {
            let mut tx = pool.begin().await.unwrap();
            let two = progress::record_step(&mut tx, org.scope(), user, b, Action::View, t0())
                .await
                .unwrap();
            tx.commit().await.unwrap();
            two
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!second.is_finished(), "the second waits for the first");
    first.commit().await.unwrap();
    let two = second.await.unwrap();
    let modules_completed = [&one, &two]
        .iter()
        .flat_map(|r| &r.events)
        .filter(|e| matches!(e, Event::ModuleCompleted(_)))
        .count();
    assert_eq!(modules_completed, 1);
    let counts: (i32, i32) = sqlx::query_as(
        "SELECT required_count, completed_count FROM module_progress WHERE user_id = $1",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (2, 2));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t7_under_one_the_first_requirement_completes_the_module(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let a = org.item(&pool, module, Some(Requirement::View)).await;
    let b = org.item(&pool, module, Some(Requirement::View)).await;
    org.rules(
        &pool,
        module,
        RulesPatch {
            completion_rule: Some(CompletionRule::One),
            ..RulesPatch::default()
        },
    )
    .await;
    let user = org.student(&pool).await;
    let first = org.step(&pool, user, a, Action::View, t0()).await.unwrap();
    assert!(
        first
            .events
            .iter()
            .any(|e| matches!(e, Event::ModuleCompleted(_)))
    );
    let second = org
        .step(&pool, user, b, Action::View, later(1))
        .await
        .unwrap();
    assert!(
        matches!(second.events.as_slice(), [Event::ItemCompleted(_)]),
        "{:?}",
        second.events
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b7_a_module_stays_complete_when_requirements_are_added(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let a = org.item(&pool, module, Some(Requirement::View)).await;
    let user = org.student(&pool).await;
    let _ = org.step(&pool, user, a, Action::View, t0()).await.unwrap();
    let b = org.item(&pool, module, Some(Requirement::View)).await;
    org.item(&pool, module, Some(Requirement::View)).await;
    let step = org
        .step(&pool, user, b, Action::View, later(1))
        .await
        .unwrap();
    assert!(
        matches!(step.events.as_slice(), [Event::ItemCompleted(_)]),
        "{:?}",
        step.events
    );
    let row: (i32, i32, bool) = sqlx::query_as(
        "SELECT required_count, completed_count, completed_at IS NOT NULL
         FROM module_progress WHERE user_id = $1",
    )
    .bind(user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row, (3, 2, true));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b5_complete_item_takes_only_the_items_requirement_kind(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::MarkDone)).await;
    let user = org.student(&pool).await;
    let mut tx = pool.begin().await.unwrap();
    let watch = Completion {
        kind: RequirementKind::Watch,
    };
    let refused = progress::complete_item(&mut tx, org.scope(), user, item, watch, t0()).await;
    assert!(
        matches!(refused, Err(Error::CompletionMismatch)),
        "{refused:?}"
    );
    let done = Completion {
        kind: RequirementKind::MarkDone,
    };
    let recorded = progress::complete_item(&mut tx, org.scope(), user, item, done, t0())
        .await
        .unwrap();
    assert_eq!(recorded.progress.state, State::Completed);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn b13_a_learner_reads_only_their_own_progress(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let item = org.item(&pool, module, Some(Requirement::View)).await;
    let (one, two) = (org.student(&pool).await, org.student(&pool).await);
    let _ = org
        .step(&pool, one, item, Action::View, t0())
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let theirs = progress::for_learner_in_course(&mut conn, org.scope(), two, org.course)
        .await
        .unwrap();
    assert_eq!(theirs, progress::LearnerProgress::default());
    let other = Org::new(&pool, "b").await;
    let cross = progress::for_learner_in_course(&mut conn, other.scope(), one, org.course)
        .await
        .unwrap();
    assert_eq!(cross, progress::LearnerProgress::default());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_an_item_with_progress_cannot_be_deleted(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let module = org.module(&pool).await;
    let started = org.item(&pool, module, Some(Requirement::MarkDone)).await;
    let untouched = org.item(&pool, module, Some(Requirement::MarkDone)).await;
    let user = org.student(&pool).await;
    let _ = org
        .step(&pool, user, started, Action::View, t0())
        .await
        .unwrap();
    let refused = items::delete(&pool, org.scope(), started).await;
    assert!(
        matches!(refused, Err(modules::Error::ItemHasProgress)),
        "{refused:?}"
    );
    items::delete(&pool, org.scope(), untouched)
        .await
        .expect("an item without progress is deleted");
}
