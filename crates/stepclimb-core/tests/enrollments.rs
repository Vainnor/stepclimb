// SPDX-License-Identifier: AGPL-3.0-or-later
//! The enrollments repository, the `enrollments` table and enrollment grants against a fresh
//! database per test (docs/specs/m1/02-courses-and-enrollments.md B4, B15 to B23, B27, Data;
//! T5, T11, T13 to T15, T21 at the repository level). Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use sqlx::PgPool;
use stepclimb_core::{
    OrgScope, Source, accounts,
    audit::{self, Actor},
    courses::{self, NewCourse, Status as CourseStatus},
    enrollments::{self, Error, ListFilter, NewEnrollment, Status},
    grants::{Grants, Target},
    id::{Uuid, new_id},
    organizations,
    perm::{CourseParticipate, CourseView, OrgManage, Permission},
    roles::{self, NewRole},
    users::{self, NewUser},
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

/// An organization with Tower and Enroute under the root, and the built-in roles synced.
#[derive(Clone, Copy)]
struct Org {
    id: Uuid,
    root: Uuid,
    tower: Uuid,
    enroute: Uuid,
}

impl Org {
    async fn new(pool: &PgPool, slug: &str) -> Self {
        roles::sync_builtin(pool, request_time())
            .await
            .expect("sync roles");
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

    async fn course(self, pool: &PgPool, account: Uuid, status: CourseStatus) -> Uuid {
        let new = NewCourse {
            account_id: account,
            title: "Course".into(),
            code: format!("C{}", &new_id().simple().to_string()[20..]),
        };
        let id = courses::create(pool, self.scope(), &new, request_time(), Actor::Instance)
            .await
            .expect("create course")
            .id;
        let mut path = vec![];
        match status {
            CourseStatus::Draft => {}
            CourseStatus::Published => path.push(CourseStatus::Published),
            CourseStatus::Archived => {
                path.extend([CourseStatus::Published, CourseStatus::Archived])
            }
        }
        for to in path {
            courses::set_status(pool, self.scope(), id, to, request_time(), Actor::Instance)
                .await
                .expect("set the course's status");
        }
        id
    }

    async fn user(self, pool: &PgPool, name: &str) -> Uuid {
        let new = NewUser {
            display_name: name.into(),
            given_name: String::new(),
            family_name: String::new(),
            email: None,
            locale: None,
            tz: "UTC".into(),
            source: Source::Local,
            external_id: None,
        };
        users::create(pool, self.scope(), &new, request_time())
            .await
            .expect("create user")
            .id
    }

    async fn custom_role(self, pool: &PgPool, permissions: &[&str]) -> Uuid {
        let new = NewRole {
            key: format!("r{}", new_id().simple()),
            name: "Custom".into(),
            permissions: permissions.iter().map(|&k| k.to_owned()).collect(),
        };
        roles::create_custom(pool, self.scope(), &new, request_time(), Actor::Instance)
            .await
            .expect("create role")
            .id
    }
}

async fn builtin(pool: &PgPool, key: &str) -> Uuid {
    roles::builtin_id(pool, key)
        .await
        .expect("read role")
        .expect("a built-in role")
}

fn new(user_id: Uuid, role_id: Uuid) -> NewEnrollment {
    NewEnrollment {
        user_id,
        role_id,
        source: Source::Local,
        external_id: None,
    }
}

async fn enroll(
    pool: &PgPool,
    org: Org,
    course: Uuid,
    user: Uuid,
    role: Uuid,
) -> Result<enrollments::Enrollment, Error> {
    enrollments::create(
        pool,
        org.scope(),
        course,
        &new(user, role),
        request_time(),
        Actor::Instance,
    )
    .await
}

async fn entries(pool: &PgPool, org: Org) -> Vec<audit::Entry> {
    audit::list(pool, org.scope(), 100).await.expect("entries")
}

/// B15, T11: one row per course, user and role; a user may hold several roles in one course.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_enrollment_is_unique_per_course_user_and_role(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let pat = a.user(&pool, "Pat").await;
    let student = builtin(&pool, "student").await;
    let instructor = builtin(&pool, "instructor").await;

    let created = enroll(&pool, a, course, pat, student).await.unwrap();
    assert_eq!(
        (
            created.status,
            created.source,
            created.user_display_name.as_str()
        ),
        (Status::Active, Source::Local, "Pat")
    );
    assert!(matches!(
        enroll(&pool, a, course, pat, student).await,
        Err(Error::Exists)
    ));
    enroll(&pool, a, course, pat, instructor).await.unwrap();
    let listed =
        enrollments::list_for_course(&pool, a.scope(), course, ListFilter::default(), None, 10)
            .await
            .unwrap();
    assert_eq!(listed.len(), 2);
}

/// Data: the table keeps the user, the course and the role in the enrollment's organization.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn another_organizations_user_course_or_role_is_refused(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let b = Org::new(&pool, "org-b").await;
    let course = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let pat = a.user(&pool, "Pat").await;
    let student = builtin(&pool, "student").await;
    let b_user = b.user(&pool, "Bo").await;
    let b_course = b.course(&pool, b.tower, CourseStatus::Draft).await;
    let b_role = b.custom_role(&pool, &[]).await;

    assert!(matches!(
        enroll(&pool, a, course, b_user, student).await,
        Err(Error::UserNotFound)
    ));
    assert!(matches!(
        enroll(&pool, a, b_course, pat, student).await,
        Err(Error::CourseNotFound)
    ));
    assert!(matches!(
        enroll(&pool, a, course, pat, b_role).await,
        Err(Error::RoleNotFound)
    ));
    // The trigger refuses what the repository's read would not let through.
    let refused = sqlx::query(
        "INSERT INTO enrollments (id, org_id, course_id, user_id, role_id)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(new_id())
    .bind(a.id)
    .bind(course)
    .bind(pat)
    .bind(b_role)
    .execute(&pool)
    .await
    .unwrap_err();
    let constraint = refused.as_database_error().and_then(|e| e.constraint());
    assert_eq!(constraint, Some("enrollments_role_in_org"));
    assert!(
        entries(&pool, a)
            .await
            .iter()
            .all(|e| !e.action.starts_with("enrollment."))
    );
}

/// B5, B17, Q5: an archived course takes no new enrollment; a template takes none whose role
/// holds `course.participate`, and does take one that holds no such permission.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn archived_courses_and_templates_refuse_learners(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let pat = a.user(&pool, "Pat").await;
    let student = builtin(&pool, "student").await;
    let designer = builtin(&pool, "course_designer").await;
    let archived = a.course(&pool, a.tower, CourseStatus::Archived).await;
    assert!(matches!(
        enroll(&pool, a, archived, pat, designer).await,
        Err(Error::CourseArchived)
    ));
    let source = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let copy = courses::CourseCopy {
        account_id: a.tower,
        title: None,
        code: "TPL".into(),
        template: true,
    };
    let template = courses::copy(
        &pool,
        a.scope(),
        source,
        &copy,
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap()
    .id;
    assert!(matches!(
        enroll(&pool, a, template, pat, student).await,
        Err(Error::Template)
    ));
    let participant = a.custom_role(&pool, &[CourseParticipate::KEY]).await;
    assert!(matches!(
        enroll(&pool, a, template, pat, participant).await,
        Err(Error::Template)
    ));
    enroll(&pool, a, template, pat, designer).await.unwrap();
}

/// B18, B27, T21: concluding and reactivating write one entry each; the current status writes
/// nothing; deleting writes one entry with the enrollment as it was.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn status_changes_and_deletes_are_audited_once(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = a.course(&pool, a.tower, CourseStatus::Published).await;
    let pat = a.user(&pool, "Pat").await;
    let student = builtin(&pool, "student").await;
    let id = enroll(&pool, a, course, pat, student).await.unwrap().id;
    let set = |to| {
        let pool = pool.clone();
        async move {
            enrollments::set_status(&pool, a.scope(), id, to, later_time(), Actor::Instance)
                .await
                .unwrap()
        }
    };
    let concluded = set(Status::Concluded).await;
    assert_eq!(concluded.status, Status::Concluded);
    assert_eq!(concluded.updated_at, later_time());
    set(Status::Concluded).await;
    enrollments::delete(&pool, a.scope(), id, later_time(), Actor::Instance)
        .await
        .unwrap();
    assert!(
        enrollments::get(&pool, a.scope(), id)
            .await
            .unwrap()
            .is_none()
    );
    let mine: Vec<_> = entries(&pool, a)
        .await
        .into_iter()
        .filter(|e| e.target_id == id)
        .map(|e| (e.action, e.data))
        .collect();
    let row = |status| {
        json!({
            "course_id": course, "user_id": pat, "role_id": student,
            "status": status, "source": "local",
        })
    };
    assert_eq!(
        mine,
        [
            (
                "enrollment.create".to_owned(),
                json!({ "after": row("active") })
            ),
            (
                "enrollment.update".to_owned(),
                json!({ "before": { "status": "active" }, "after": { "status": "concluded" } })
            ),
            (
                "enrollment.delete".to_owned(),
                json!({ "before": row("concluded") })
            ),
        ]
    );
    assert!(matches!(
        enrollments::delete(&pool, a.scope(), id, later_time(), Actor::Instance).await,
        Err(Error::NotFound)
    ));
}

/// B4, T5: a published course with an active learner cannot go back to draft; a concluded one,
/// or an active enrollment whose role does not hold `course.participate`, does not stop it.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_course_with_learners_stays_published(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = a.course(&pool, a.tower, CourseStatus::Published).await;
    let pat = a.user(&pool, "Pat").await;
    let to_draft = || {
        let pool = pool.clone();
        async move {
            courses::set_status(
                &pool,
                a.scope(),
                course,
                CourseStatus::Draft,
                later_time(),
                Actor::Instance,
            )
            .await
        }
    };
    let designer = builtin(&pool, "course_designer").await;
    enroll(&pool, a, course, pat, designer).await.unwrap();
    let student = builtin(&pool, "student").await;
    let learner = enroll(&pool, a, course, pat, student).await.unwrap();
    assert!(matches!(to_draft().await, Err(courses::Error::HasLearners)));
    enrollments::set_status(
        &pool,
        a.scope(),
        learner.id,
        Status::Concluded,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(to_draft().await.unwrap().status, CourseStatus::Draft);
}

/// B16, B23, T13, T14: an active enrollment grants its role's course-scope permissions at its
/// course only; a concluded one grants nothing; a permission not grantable at course scope has
/// no effect through an enrollment.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_active_enrollment_is_a_course_grant(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = a.course(&pool, a.tower, CourseStatus::Published).await;
    let other = a.course(&pool, a.tower, CourseStatus::Published).await;
    let pat = a.user(&pool, "Pat").await;
    let role = a
        .custom_role(&pool, &[OrgManage::KEY, CourseView::KEY])
        .await;
    let id = enroll(&pool, a, course, pat, role).await.unwrap().id;
    let at = |course_id| Target::Course {
        account_path: vec![a.root, a.tower],
        course_id,
    };
    let grants = Grants::load(&pool, a.scope(), pat).await.unwrap();
    assert!(grants.holds::<CourseView>(&at(course)));
    assert!(!grants.is_visible(&at(other)));
    assert!(!grants.holds_anywhere::<OrgManage>());
    assert_eq!(grants.courses_with::<CourseView>(), [course]);

    enrollments::set_status(
        &pool,
        a.scope(),
        id,
        Status::Concluded,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let grants = Grants::load(&pool, a.scope(), pat).await.unwrap();
    assert!(!grants.is_visible(&at(course)));
    assert!(
        enrollments::get(&pool, a.scope(), id)
            .await
            .unwrap()
            .is_some(),
        "the row stays as history"
    );
}

/// Spec 09 B10: a custom role with an enrollment cannot be deleted.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_role_with_an_enrollment_is_in_use(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let pat = a.user(&pool, "Pat").await;
    let role = a.custom_role(&pool, &[]).await;
    let id = enroll(&pool, a, course, pat, role).await.unwrap().id;
    let delete = || roles::delete_custom(&pool, a.scope(), role, later_time(), Actor::Instance);
    assert!(matches!(delete().await, Err(roles::Error::InUse)));
    enrollments::delete(&pool, a.scope(), id, later_time(), Actor::Instance)
        .await
        .unwrap();
    delete().await.unwrap();
}

/// B20: the list filters by status, role and user and pages by display name, then id.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_list_filters_and_pages_by_display_name(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let elsewhere = a.course(&pool, a.enroute, CourseStatus::Draft).await;
    let student = builtin(&pool, "student").await;
    let instructor = builtin(&pool, "instructor").await;
    let (sam, ari, lee) = (
        a.user(&pool, "Sam").await,
        a.user(&pool, "Ari").await,
        a.user(&pool, "Lee").await,
    );
    enroll(&pool, a, course, sam, student).await.unwrap();
    let ari_id = enroll(&pool, a, course, ari, instructor).await.unwrap().id;
    let lee_id = enroll(&pool, a, course, lee, student).await.unwrap().id;
    enroll(&pool, a, elsewhere, sam, student).await.unwrap();
    enrollments::set_status(
        &pool,
        a.scope(),
        lee_id,
        Status::Concluded,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let list = |filter, after: Option<(String, Uuid)>| {
        let pool = pool.clone();
        async move {
            enrollments::list_for_course(&pool, a.scope(), course, filter, after.as_ref(), 2)
                .await
                .unwrap()
                .into_iter()
                .map(|e| e.user_display_name)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(list(ListFilter::default(), None).await, ["Ari", "Lee"]);
    assert_eq!(
        list(ListFilter::default(), Some(("Ari".into(), ari_id))).await,
        ["Lee", "Sam"]
    );
    let active = ListFilter {
        status: Some(Status::Active),
        ..ListFilter::default()
    };
    assert_eq!(list(active, None).await, ["Ari", "Sam"]);
    let students = ListFilter {
        role_id: Some(student),
        user_id: Some(sam),
        ..ListFilter::default()
    };
    assert_eq!(list(students, None).await, ["Sam"]);
    assert!(matches!(
        enrollments::list_for_course(
            &pool,
            a.scope(),
            course,
            ListFilter::default(),
            Some(&("A\0".into(), ari_id)),
            2
        )
        .await,
        Err(Error::ContainsNul { field: "cursor" })
    ));
}

/// B21: a user actively enrolled in a course of an account is "in" that account for an
/// account-scope `user.view`; one enrolled elsewhere, or only concluded there, is not.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_enrollment_puts_a_user_in_the_courses_account(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let in_tower = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let in_enroute = a.course(&pool, a.enroute, CourseStatus::Draft).await;
    let student = builtin(&pool, "student").await;
    let (sam, lee, kim) = (
        a.user(&pool, "Sam").await,
        a.user(&pool, "Lee").await,
        a.user(&pool, "Kim").await,
    );
    enroll(&pool, a, in_tower, sam, student).await.unwrap();
    enroll(&pool, a, in_enroute, lee, student).await.unwrap();
    // A concluded enrollment puts no one in the account (owner decision of 2026-11-17).
    let kim_there = enroll(&pool, a, in_tower, kim, student).await.unwrap().id;
    enrollments::set_status(
        &pool,
        a.scope(),
        kim_there,
        Status::Concluded,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let list = |within: Option<Vec<Uuid>>| {
        let pool = pool.clone();
        async move {
            let filter = users::ListFilter {
                within,
                ..users::ListFilter::default()
            };
            users::list(&pool, a.scope(), &filter, None, 10)
                .await
                .unwrap()
                .into_iter()
                .map(|u| u.id)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(list(Some(vec![a.tower])).await, [sam]);
    assert_eq!(
        list(Some(vec![a.root])).await,
        [lee, sam],
        "by display name"
    );
    assert!(list(Some(vec![])).await.is_empty());
    assert_eq!(list(None).await, [kim, lee, sam]);
    let covers = |user, within: Option<Vec<Uuid>>| {
        let pool = pool.clone();
        async move { users::covers(&pool, a.scope(), user, within).await.unwrap() }
    };
    assert!(covers(sam, Some(vec![a.tower])).await);
    assert!(!covers(lee, Some(vec![a.tower])).await);
    assert!(!covers(kim, Some(vec![a.tower])).await);
    assert!(covers(kim, None).await);
    assert!(!covers(new_id(), None).await);
}

/// Q5: reactivating a concluded learner enrollment in a template is refused as creating one
/// is, and changes nothing; concluding one, or reactivating a non-learner role, is allowed.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn reactivating_a_learner_in_a_template_is_refused(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let source = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let copy = courses::CourseCopy {
        account_id: a.tower,
        title: None,
        code: "TPL".into(),
        template: true,
    };
    let template = courses::copy(
        &pool,
        a.scope(),
        source,
        &copy,
        request_time(),
        Actor::Instance,
    )
    .await
    .unwrap()
    .id;
    let pat = a.user(&pool, "Pat").await;
    let student = builtin(&pool, "student").await;
    let id = new_id();
    sqlx::query(
        "INSERT INTO enrollments (id, org_id, course_id, user_id, role_id, status)
         VALUES ($1, $2, $3, $4, $5, 'concluded')",
    )
    .bind(id)
    .bind(a.id)
    .bind(template)
    .bind(pat)
    .bind(student)
    .execute(&pool)
    .await
    .unwrap();
    let set = |id, to| {
        let pool = pool.clone();
        async move {
            enrollments::set_status(&pool, a.scope(), id, to, later_time(), Actor::Instance).await
        }
    };
    assert!(matches!(
        set(id, Status::Active).await,
        Err(Error::Template)
    ));
    let stored = enrollments::get(&pool, a.scope(), id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.status, Status::Concluded);
    let designer = builtin(&pool, "course_designer").await;
    let staff = enroll(&pool, a, template, pat, designer).await.unwrap().id;
    set(staff, Status::Concluded).await.unwrap();
    set(staff, Status::Active).await.unwrap();
}

/// `m0/03` B6's serialization with enrollments: `roles::key_share_custom_of_user` locks the
/// custom roles of the user's enrollments too, so a change of such a role waits for the
/// caller's transaction.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_users_escalation_lock_covers_enrollment_roles(pool: PgPool) {
    let a = Org::new(&pool, "org-a").await;
    let course = a.course(&pool, a.tower, CourseStatus::Draft).await;
    let pat = a.user(&pool, "Pat").await;
    let role = a.custom_role(&pool, &[CourseView::KEY]).await;
    enroll(&pool, a, course, pat, role).await.unwrap();

    let mut checking = pool.begin().await.unwrap();
    roles::key_share_custom_of_user(&mut checking, a.scope(), pat)
        .await
        .unwrap();
    let mut changing = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout = '200ms'")
        .execute(&mut *changing)
        .await
        .unwrap();
    let locked = roles::lock_custom(&mut changing, a.scope(), role).await;
    assert!(locked.is_err(), "the role change waits: {locked:?}");
    drop(changing);
    checking.commit().await.unwrap();
    let mut changing = pool.begin().await.unwrap();
    roles::lock_custom(&mut changing, a.scope(), role)
        .await
        .expect("free once the check commits");
}
