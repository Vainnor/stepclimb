// SPDX-License-Identifier: AGPL-3.0-or-later
//! Assignments at the repository, against a fresh database per test
//! (docs/specs/m1/13-assignments-due-dates-reminders-and-escalation.md B1 to B5, B7 to B9; T1
//! to T3 and T8's due changes at the repository). The routes, the permission cases and the
//! completion subscriber are in the server's `tests/assignments.rs` and `tests/permissions.rs`.
//! Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, OrgSettingsPatch, Source,
    assignments::{self, Completed, Counts, DueRule, Enroll, Error, NewAssignment, TargetType},
    audit::Actor,
    courses::{self, NewCourse},
    enrollments::{self, NewEnrollment},
    id::Uuid,
    items::{self, ItemPatch, Kind, NewItem},
    modules::{self, ModulePatch},
    organizations,
    progress::{self, Action},
    roles,
    rules::Requirement,
    users::{self, NewUser},
};

/// 2026-03-01T10:00:00Z.
fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_772_359_200)
}

fn days(n: u64) -> Duration {
    Duration::from_secs(n * 86_400)
}

fn at(text: &str) -> SystemTime {
    courses::parse_timestamp(text).expect("an RFC 3339 time")
}

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the credential resolvers"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An organization with a published course holding a published module with one published
/// link item (complete on view), and an admin user who creates assignments.
#[derive(Clone, Copy)]
struct Org {
    id: Uuid,
    course: Uuid,
    module: Uuid,
    item: Uuid,
    admin: Uuid,
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
        let module = modules::create(pool, scope(id), course, "Module", t0())
            .await
            .expect("create module")
            .id;
        let publish = ModulePatch {
            published: Some(true),
            ..ModulePatch::default()
        };
        modules::update(pool, scope(id), module, &publish, t0())
            .await
            .expect("publish module");
        let new_item = NewItem {
            kind: Kind::ExternalLink,
            title: "Link title".into(),
            ref_id: None,
            url: Some("https://example.com/".into()),
            position: None,
        };
        let item = items::create(pool, scope(id), module, &new_item, t0())
            .await
            .expect("create item")
            .id;
        let publish_item = ItemPatch {
            published: Some(true),
            ..ItemPatch::default()
        };
        items::update(pool, scope(id), item, &publish_item, t0())
            .await
            .expect("publish item");
        items::set_requirement(pool, scope(id), item, Some(Requirement::View), t0())
            .await
            .expect("set requirement");
        let mut org = Self {
            id,
            course,
            module,
            item,
            admin: Uuid::nil(),
        };
        org.admin = org.user(pool, "Admin Person").await;
        courses::set_status(
            pool,
            scope(id),
            course,
            courses::Status::Published,
            t0(),
            Actor::Instance,
        )
        .await
        .expect("publish course");
        org
    }

    fn scope(self) -> OrgScope {
        scope(self.id)
    }

    async fn user(self, pool: &PgPool, name: &str) -> Uuid {
        let user = NewUser {
            display_name: name.into(),
            given_name: String::new(),
            family_name: String::new(),
            email: Some(format!("{}@example.test", Uuid::now_v7().simple())),
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        users::create(pool, self.scope(), &user, t0())
            .await
            .expect("create user")
            .id
    }

    async fn enroll(self, pool: &PgPool, user: Uuid, role: &str, source: Source) -> Uuid {
        let role_id = roles::builtin_id(pool, role)
            .await
            .expect("read role")
            .expect("a built-in role");
        let new = NewEnrollment {
            user_id: user,
            role_id,
            source,
            external_id: (source != Source::Local).then(|| "ext-1".to_owned()),
            observed_user_ids: Vec::new(),
        };
        enrollments::create(pool, self.scope(), self.course, &new, t0(), Actor::Instance)
            .await
            .expect("enroll")
            .id
    }

    async fn student(self, pool: &PgPool) -> Uuid {
        let user = self.user(pool, "Student Person").await;
        self.enroll(pool, user, "student", Source::Local).await;
        user
    }

    fn assignment(
        self,
        target_type: TargetType,
        user: Uuid,
        due: Option<DueRule>,
    ) -> NewAssignment {
        let target_id = match target_type {
            TargetType::Course => self.course,
            TargetType::Module => self.module,
            TargetType::Item => self.item,
        };
        NewAssignment {
            target_type,
            target_id,
            user_id: user,
            due,
            created_by: self.admin,
        }
    }

    async fn assign(
        self,
        pool: &PgPool,
        new: &NewAssignment,
        enroll: Enroll,
        now: SystemTime,
    ) -> Result<assignments::Assignment, Error> {
        assignments::create(pool, self.scope(), self.course, new, enroll, now).await
    }

    async fn get(self, pool: &PgPool, id: Uuid, now: SystemTime) -> assignments::Assignment {
        let mut conn = pool.acquire().await.expect("a connection");
        assignments::get(&mut conn, self.scope(), id, now)
            .await
            .expect("read")
            .expect("the assignment")
    }

    async fn view(self, pool: &PgPool, user: Uuid, now: SystemTime) -> progress::Recorded {
        let mut tx = pool.begin().await.expect("begin");
        let recorded =
            progress::record_step(&mut tx, self.scope(), user, self.item, Action::View, now)
                .await
                .expect("a view");
        tx.commit().await.expect("commit");
        recorded
    }

    async fn set_tz(self, pool: &PgPool, tz: &str) {
        let patch = OrgSettingsPatch {
            default_tz: Some(tz.into()),
            ..OrgSettingsPatch::default()
        };
        organizations::update_settings(pool, self.scope(), &patch, t0(), Actor::Instance)
            .await
            .expect("set the time zone");
    }
}

async fn row_due(pool: &PgPool, assignment: Uuid) -> Option<SystemTime> {
    let micros: Option<i64> = sqlx::query_scalar(
        "SELECT (extract(epoch FROM due_at) * 1000000)::bigint
         FROM assignment_users WHERE assignment_id = $1",
    )
    .bind(assignment)
    .fetch_one(pool)
    .await
    .expect("the row");
    micros.map(|m| UNIX_EPOCH + Duration::from_micros(u64::try_from(m).expect("after 1970")))
}

async fn active_student_enrollments(pool: &PgPool, user: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM enrollments e JOIN roles r ON r.id = e.role_id
         WHERE e.user_id = $1 AND r.key = 'student' AND e.status = 'active'",
    )
    .bind(user)
    .fetch_one(pool)
    .await
    .expect("count")
}

fn counts(not_started: u32, in_progress: u32, completed: u32, overdue: u32) -> Counts {
    Counts {
        not_started,
        in_progress,
        completed,
        overdue,
    }
}

/// T1 at the repository: an unenrolled user is enrolled as Student when the caller may, and
/// nothing is written when they may not.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t1_assigning_an_unenrolled_user_enrolls_them_only_when_allowed(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let pat = org.user(&pool, "Pat").await;
    let new = org.assignment(TargetType::Course, pat, None);

    let refused = org.assign(&pool, &new, Enroll::Refuse, t0()).await;
    assert!(
        matches!(refused, Err(Error::EnrollmentNeeded)),
        "{refused:?}"
    );
    assert_eq!(active_student_enrollments(&pool, pat).await, 0);
    let none: i64 = sqlx::query_scalar("SELECT count(*) FROM assignments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(none, 0);

    let actor = Actor::User {
        id: org.admin,
        ip: std::net::IpAddr::from([192, 0, 2, 10]),
    };
    let created = org
        .assign(&pool, &new, Enroll::AsStudent(actor), t0())
        .await
        .unwrap();
    assert_eq!(active_student_enrollments(&pool, pat).await, 1);
    assert_eq!(created.user_id, pat);
    assert_eq!(created.target_type, TargetType::Course);
    assert_eq!(created.target_title, "Course");
    assert_eq!(created.counts, counts(1, 0, 0, 0));
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'enrollment.create' AND actor_id = $1",
    )
    .bind(org.admin)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 1, "the enrollment is audited as the caller's");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_learner_is_assigned_without_a_new_enrollment(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let student = org.student(&pool).await;
    let new = org.assignment(TargetType::Item, student, None);
    let created = org.assign(&pool, &new, Enroll::Refuse, t0()).await.unwrap();
    assert_eq!(created.target_title, "Link title");
    assert_eq!(active_student_enrollments(&pool, student).await, 1);
}

/// A concluded `local` Student enrollment is reactivated; one an outside source owns refuses.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_concluded_student_enrollment_is_reactivated_unless_owned_elsewhere(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let actor = Actor::Instance;
    for (source, reactivated) in [(Source::Local, true), (Source::Idp, false)] {
        let user = org.user(&pool, "Concluded").await;
        let enrollment = org.enroll(&pool, user, "student", source).await;
        enrollments::set_status(
            &pool,
            org.scope(),
            enrollment,
            enrollments::Status::Concluded,
            t0(),
            actor,
        )
        .await
        .unwrap();
        let new = org.assignment(TargetType::Course, user, None);
        let result = org
            .assign(&pool, &new, Enroll::AsStudent(actor), t0())
            .await;
        if reactivated {
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(active_student_enrollments(&pool, user).await, 1);
        } else {
            assert!(
                matches!(result, Err(Error::EnrollmentExternallyManaged)),
                "{result:?}"
            );
            assert_eq!(active_student_enrollments(&pool, user).await, 0);
        }
    }
}

/// B3: an archived course and a template take no assignment.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_archived_course_or_a_template_refuses(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let student = org.student(&pool).await;
    let new = org.assignment(TargetType::Course, student, None);
    courses::set_status(
        &pool,
        org.scope(),
        org.course,
        courses::Status::Archived,
        t0(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let archived = org.assign(&pool, &new, Enroll::Refuse, t0()).await;
    assert!(
        matches!(archived, Err(Error::CourseArchived)),
        "{archived:?}"
    );
    sqlx::query("UPDATE courses SET status = 'draft', template = true WHERE id = $1")
        .bind(org.course)
        .execute(&pool)
        .await
        .unwrap();
    let template = org.assign(&pool, &new, Enroll::Refuse, t0()).await;
    assert!(
        matches!(template, Err(Error::CourseTemplate)),
        "{template:?}"
    );
}

/// The target is the course itself or a module or item of it; another course's is refused.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_target_outside_the_course_is_refused(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let other = Org::new(&pool, "b").await;
    let student = org.student(&pool).await;
    for (target_type, target_id) in [
        (TargetType::Course, other.course),
        (TargetType::Module, other.module),
        (TargetType::Item, other.item),
        (TargetType::Module, org.item),
        (TargetType::Item, org.module),
    ] {
        let new = NewAssignment {
            target_id,
            ..org.assignment(target_type, student, None)
        };
        let refused = org.assign(&pool, &new, Enroll::Refuse, t0()).await;
        assert!(
            matches!(refused, Err(Error::TargetNotFound(t)) if t == target_type),
            "{target_type:?}: {refused:?}"
        );
    }
    let other_user = other.user(&pool, "Other").await;
    let new = org.assignment(TargetType::Course, other_user, None);
    let refused = org.assign(&pool, &new, Enroll::Refuse, t0()).await;
    assert!(matches!(refused, Err(Error::UserNotFound)), "{refused:?}");
}

/// T2: a relative due date ends its last local day in the organization's time zone; a fixed
/// one is kept as given.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t2_due_dates_follow_the_rule_and_the_organization_time_zone(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    org.set_tz(&pool, "America/New_York").await;
    let student = org.student(&pool).await;
    let relative = org.assignment(TargetType::Course, student, Some(DueRule::RelativeDays(14)));
    let created = org
        .assign(&pool, &relative, Enroll::Refuse, t0())
        .await
        .unwrap();
    assert_eq!(created.due, Some(DueRule::RelativeDays(14)));
    assert_eq!(
        row_due(&pool, created.id).await,
        Some(at("2026-03-15T23:59:59-04:00"))
    );
    let fixed_at = at("2026-04-01T12:00:00Z");
    let fixed = org.assignment(TargetType::Module, student, Some(DueRule::Fixed(fixed_at)));
    let created = org
        .assign(&pool, &fixed, Enroll::Refuse, t0())
        .await
        .unwrap();
    assert_eq!(created.due, Some(DueRule::Fixed(fixed_at)));
    assert_eq!(row_due(&pool, created.id).await, Some(fixed_at));
}

/// T3: not started, in progress after a view of another item, overdue after the due time, then
/// completed (late) when the course completes through the progress event's data.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_a_row_moves_through_its_statuses(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let student = org.student(&pool).await;
    let due = DueRule::Fixed(t0() + days(2));
    let new = org.assignment(TargetType::Module, student, Some(due));
    let created = org.assign(&pool, &new, Enroll::Refuse, t0()).await.unwrap();
    assert_eq!(created.counts, counts(1, 0, 0, 0));

    // A second item that needs marking done: viewing it starts it without completing it.
    let mark = NewItem {
        kind: Kind::ExternalLink,
        title: "Mark".into(),
        ref_id: None,
        url: Some("https://example.com/mark".into()),
        position: None,
    };
    let second = items::create(&pool, org.scope(), org.module, &mark, t0())
        .await
        .unwrap()
        .id;
    let publish = ItemPatch {
        published: Some(true),
        ..ItemPatch::default()
    };
    items::update(&pool, org.scope(), second, &publish, t0())
        .await
        .unwrap();
    items::set_requirement(
        &pool,
        org.scope(),
        second,
        Some(Requirement::MarkDone),
        t0(),
    )
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let started = progress::record_step(&mut tx, org.scope(), student, second, Action::View, t0())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(started.events.is_empty(), "started, not completed");
    let now = t0() + days(1);
    assert_eq!(
        org.get(&pool, created.id, now).await.counts,
        counts(0, 1, 0, 0)
    );
    let late = t0() + days(3);
    assert_eq!(
        org.get(&pool, created.id, late).await.counts,
        counts(0, 0, 0, 1)
    );

    let module_done = Completed {
        user_id: student,
        target_type: TargetType::Module,
        target_id: org.module,
        completed_at: late,
    };
    let done = assignments::record_completion(&pool, org.scope(), &module_done, late)
        .await
        .unwrap();
    assert_eq!(done, 1);
    let read = org.get(&pool, created.id, late + days(1)).await;
    assert_eq!(
        read.counts,
        counts(0, 0, 1, 0),
        "completed late is completed (B9)"
    );
    let again = assignments::record_completion(&pool, org.scope(), &module_done, late)
        .await
        .unwrap();
    assert_eq!(again, 0, "a rerun completes nothing more");
}

/// B8, Q5: a target completed before the assignment completes the row at once; a completion
/// of another target or user completes nothing.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t3_an_earlier_completion_completes_the_row_at_once(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let student = org.student(&pool).await;
    let recorded = org.view(&pool, student, t0()).await;
    assert!(!recorded.events.is_empty(), "the view completes the item");
    for target_type in [TargetType::Item, TargetType::Module, TargetType::Course] {
        let new = org.assignment(target_type, student, Some(DueRule::RelativeDays(7)));
        let created = org
            .assign(&pool, &new, Enroll::Refuse, t0() + days(1))
            .await
            .unwrap();
        assert_eq!(created.counts, counts(0, 0, 1, 0), "{target_type:?}");
    }
    let other = org.student(&pool).await;
    let new = org.assignment(TargetType::Item, other, None);
    let created = org.assign(&pool, &new, Enroll::Refuse, t0()).await.unwrap();
    let elsewhere = Completed {
        user_id: student,
        target_type: TargetType::Item,
        target_id: org.item,
        completed_at: t0(),
    };
    assignments::record_completion(&pool, org.scope(), &elsewhere, t0())
        .await
        .unwrap();
    assert_eq!(
        org.get(&pool, created.id, t0()).await.counts,
        counts(1, 0, 0, 0)
    );
}

/// T8's due changes: a new rule recomputes the due date of rows not completed; a completed row
/// keeps its own; clearing the rule clears it; an ended assignment refuses a change.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t8_changing_the_due_rule_recomputes_open_rows_only(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let open = org.student(&pool).await;
    let done = org.student(&pool).await;
    let first = DueRule::Fixed(t0() + days(1));
    let open_id = org
        .assign(
            &pool,
            &org.assignment(TargetType::Item, open, Some(first)),
            Enroll::Refuse,
            t0(),
        )
        .await
        .unwrap()
        .id;
    let done_id = org
        .assign(
            &pool,
            &org.assignment(TargetType::Item, done, Some(first)),
            Enroll::Refuse,
            t0(),
        )
        .await
        .unwrap()
        .id;
    let _completed = org.view(&pool, done, t0() + days(2)).await;
    let completed = Completed {
        user_id: done,
        target_type: TargetType::Item,
        target_id: org.item,
        completed_at: t0() + days(2),
    };
    assignments::record_completion(&pool, org.scope(), &completed, t0() + days(2))
        .await
        .unwrap();

    let later = DueRule::RelativeDays(10);
    for id in [open_id, done_id] {
        let changed = assignments::set_due(&pool, org.scope(), id, Some(later), t0())
            .await
            .unwrap();
        assert_eq!(changed.due, Some(later));
    }
    assert_eq!(
        row_due(&pool, open_id).await,
        Some(at("2026-03-11T23:59:59Z"))
    );
    assert_eq!(
        row_due(&pool, done_id).await,
        Some(t0() + days(1)),
        "the completed row keeps the due date it was completed against"
    );
    let cleared = assignments::set_due(&pool, org.scope(), open_id, None, t0())
        .await
        .unwrap();
    assert_eq!(cleared.due, None);
    assert_eq!(row_due(&pool, open_id).await, None);

    let ended = assignments::end(&pool, org.scope(), open_id, t0() + days(3))
        .await
        .unwrap();
    assert_eq!(ended.ended_at, Some(t0() + days(3)));
    let again = assignments::end(&pool, org.scope(), open_id, t0() + days(4))
        .await
        .unwrap();
    assert_eq!(
        again.ended_at,
        Some(t0() + days(3)),
        "ending again changes nothing"
    );
    let refused = assignments::set_due(&pool, org.scope(), open_id, Some(later), t0()).await;
    assert!(matches!(refused, Err(Error::Ended)), "{refused:?}");
}

/// The list holds the course's assignments newest first, ended ones on request, by page.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_list_is_newest_first_and_leaves_out_ended_ones(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let student = org.student(&pool).await;
    let mut ids = Vec::new();
    for target_type in [TargetType::Course, TargetType::Module, TargetType::Item] {
        let new = org.assignment(target_type, student, None);
        ids.push(
            org.assign(&pool, &new, Enroll::Refuse, t0())
                .await
                .unwrap()
                .id,
        );
    }
    assignments::end(&pool, org.scope(), ids[1], t0())
        .await
        .unwrap();
    let list = |include_ended, after, limit| listed(&pool, org, include_ended, after, limit);
    assert_eq!(list(false, None, 10).await, [ids[2], ids[0]]);
    assert_eq!(list(true, None, 10).await, [ids[2], ids[1], ids[0]]);
    assert_eq!(list(true, Some(ids[2]), 1).await, [ids[1]]);
    let mut conn = pool.acquire().await.unwrap();
    let other = Org::new(&pool, "b").await;
    let foreign =
        assignments::list_for_course(&mut conn, other.scope(), org.course, true, None, 10, t0())
            .await
            .unwrap();
    assert!(
        foreign.is_empty(),
        "another organization's scope reads none"
    );
}

/// The ids of `org`'s course's assignments as `list_for_course` pages them at `t0`.
async fn listed(
    pool: &PgPool,
    org: Org,
    include_ended: bool,
    after: Option<Uuid>,
    limit: i64,
) -> Vec<Uuid> {
    let mut conn = pool.acquire().await.expect("a connection");
    assignments::list_for_course(
        &mut conn,
        org.scope(),
        org.course,
        include_ended,
        after,
        limit,
        t0(),
    )
    .await
    .expect("list")
    .into_iter()
    .map(|a| a.id)
    .collect()
}

/// Every read and write is org-scoped: org B's scope neither reads nor changes org A's
/// assignment, and org B's completion of the same ids completes nothing of A's.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organization_reads_and_changes_nothing(pool: PgPool) {
    let a = Org::new(&pool, "a").await;
    let b = Org::new(&pool, "b").await;
    let student = a.student(&pool).await;
    let id = a
        .assign(
            &pool,
            &a.assignment(TargetType::Item, student, None),
            Enroll::Refuse,
            t0(),
        )
        .await
        .unwrap()
        .id;
    let mut conn = pool.acquire().await.unwrap();
    assert!(
        assignments::get(&mut conn, b.scope(), id, t0())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        assignments::course_of(&pool, b.scope(), id)
            .await
            .unwrap()
            .is_none()
    );
    let changed = assignments::set_due(&pool, b.scope(), id, None, t0()).await;
    assert!(matches!(changed, Err(Error::NotFound)), "{changed:?}");
    let ended = assignments::end(&pool, b.scope(), id, t0()).await;
    assert!(matches!(ended, Err(Error::NotFound)), "{ended:?}");
    let completed = Completed {
        user_id: student,
        target_type: TargetType::Item,
        target_id: a.item,
        completed_at: t0(),
    };
    let done = assignments::record_completion(&pool, b.scope(), &completed, t0())
        .await
        .unwrap();
    assert_eq!(done, 0);
    let new = a.assignment(TargetType::Course, student, None);
    let created = assignments::create(&pool, b.scope(), a.course, &new, Enroll::Refuse, t0()).await;
    assert!(matches!(created, Err(Error::CourseNotFound)), "{created:?}");
    let read = a.get(&pool, id, t0()).await;
    assert_eq!(read.ended_at, None);
    assert_eq!(read.counts, counts(1, 0, 0, 0));
}

/// Data: a module or item an assignment names, ended or not, cannot be deleted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_assigned_module_or_item_cannot_be_deleted(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let student = org.student(&pool).await;
    let item = org
        .assign(
            &pool,
            &org.assignment(TargetType::Item, student, None),
            Enroll::Refuse,
            t0(),
        )
        .await
        .unwrap();
    assignments::end(&pool, org.scope(), item.id, t0())
        .await
        .unwrap();
    let refused = items::delete(&pool, org.scope(), org.item).await;
    assert!(
        matches!(refused, Err(modules::Error::ItemAssigned)),
        "{refused:?}"
    );

    let empty = modules::create(&pool, org.scope(), org.course, "Empty", t0())
        .await
        .unwrap()
        .id;
    let new = NewAssignment {
        target_id: empty,
        ..org.assignment(TargetType::Module, student, None)
    };
    org.assign(&pool, &new, Enroll::Refuse, t0()).await.unwrap();
    let refused = modules::delete(&pool, org.scope(), empty).await;
    assert!(
        matches!(refused, Err(modules::Error::ModuleAssigned)),
        "{refused:?}"
    );

    let unassigned = modules::create(&pool, org.scope(), org.course, "Free", t0())
        .await
        .unwrap()
        .id;
    modules::delete(&pool, org.scope(), unassigned)
        .await
        .expect("an unassigned empty module is deleted");
}

/// Coordinator decision (2026-11-19): a concluded enrollment an outside source owns blocks
/// enrolling through an assignment whenever its role holds `course.participate`, whatever the
/// role; one whose role does not (an Observer) does not block.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_participating_enrollment_concluded_elsewhere_blocks_any_role(pool: PgPool) {
    let org = Org::new(&pool, "a").await;
    let actor = Actor::Instance;
    let custom = roles::NewRole {
        key: "synced_learner".into(),
        name: "Synced learner".into(),
        permissions: vec!["course.participate".into()],
    };
    let participant = roles::create_custom(&pool, org.scope(), &custom, t0(), actor)
        .await
        .expect("create role")
        .id;
    let observer = roles::builtin_id(&pool, "observer")
        .await
        .expect("read role")
        .expect("Observer");
    for (role, blocks) in [(participant, true), (observer, false)] {
        let user = org.user(&pool, "Synced").await;
        sqlx::query(
            "INSERT INTO enrollments (id, org_id, course_id, user_id, role_id, status, source,
                                      external_id)
             VALUES ($1, $2, $3, $4, $5, 'concluded', 'scim', 'ext-1')",
        )
        .bind(Uuid::now_v7())
        .bind(org.id)
        .bind(org.course)
        .bind(user)
        .bind(role)
        .execute(&pool)
        .await
        .expect("insert a SCIM enrollment");
        let new = org.assignment(TargetType::Course, user, None);
        let result = org
            .assign(&pool, &new, Enroll::AsStudent(actor), t0())
            .await;
        if blocks {
            assert!(
                matches!(result, Err(Error::EnrollmentExternallyManaged)),
                "{result:?}"
            );
            assert_eq!(active_student_enrollments(&pool, user).await, 0);
        } else {
            assert!(result.is_ok(), "{result:?}");
            assert_eq!(active_student_enrollments(&pool, user).await, 1);
        }
    }
}
