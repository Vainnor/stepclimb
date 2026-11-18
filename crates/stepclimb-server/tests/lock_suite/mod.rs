// SPDX-License-Identifier: AGPL-3.0-or-later
//! The lock suite (docs/specs/m1/04-unlock-and-completion-rules.md B13, T13), run with the
//! permission suite so it shares its walker, its setups and its organizations.
//!
//! Every operation of the OpenAPI document whose path names an item or what an item points at
//! (a path parameter in [`CONTENT_PARAMS`], or ending in `_` and one of them) has an entry in
//! [`LOCK_TABLE`]; the walker test fails, naming each, on one without. From each entry:
//!
//! - **checked** (a route that hands a learner an item's content or accepts progress for it): a
//!   Student whose item is locked gets 403 `item.locked` with its reasons, and the
//!   organization's rows are unchanged;
//! - **read** (`GET /api/v1/items/{item_id}`): the same Student gets 200 with the item's lock
//!   and without what it points at (B12);
//! - **staff** (a route only staff call): its permission suite entry requires a permission in
//!   [`STAFF`], which neither the built-in Student nor the Observer holds, and a Student
//!   enrolled in the target's course gets 403 `permission.denied`.

use super::*;

/// The path parameters that name an item or what an item points at. Later content specs add
/// their ids (a quiz's, an activity's) here and their routes to [`LOCK_TABLE`].
const CONTENT_PARAMS: &[&str] = &["item_id", "page_id", "file_id", "video_id"];

/// The permissions a staff-only content route may require: authors see an item's content
/// whatever its lock (B5).
const STAFF: &[&str] = &[CourseViewAll::KEY, CourseContentEdit::KEY];

/// How a content route keeps a locked item's content from a learner.
enum Lock {
    /// It checks the lock: 403 `item.locked`. The setup builds a published item, locked for a
    /// Student, and the request; the Student is enrolled at the call's `at`.
    Checked(Setup),
    /// It answers a locked item with B12's fields.
    Read(Setup),
    /// Only staff may call it; the permission suite's entry has its setup.
    Staff,
}

struct LockEntry {
    operation_id: &'static str,
    lock: Lock,
}

const LOCK_TABLE: &[LockEntry] = &[
    LockEntry {
        operation_id: "recordItemProgress",
        lock: Lock::Checked(view_locked_link),
    },
    LockEntry {
        operation_id: "getItem",
        lock: Lock::Read(view_locked_link),
    },
    LockEntry {
        operation_id: "updateItem",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "deleteItem",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "moveItem",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "getCourseFile",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "updateCourseFile",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "deleteCourseFile",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "updatePage",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "deletePage",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "savePageBody",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "listPageVersions",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "getPageVersion",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "restorePageVersion",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "getVideo",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "updateVideo",
        lock: Lock::Staff,
    },
];

/// The URL of the suite's links ([`Org::link`]).
const LINK_URL: &str = "https://example.com/suite";

/// A published course in Tower whose published module opens in a day, holding a published link
/// with a `view` requirement: the link is locked by its module. The request views it.
fn view_locked_link(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (course, module, item) = outline(org).await;
        let dated = modules::RulesPatch {
            unlock_at: Some(Some(now() + Duration::from_secs(86_400))),
            ..modules::RulesPatch::default()
        };
        modules::set_rules(&org.pool, scope(org.id), module, &dated, now())
            .await
            .expect("set the unlock date");
        let view = Some(stepclimb_core::rules::Requirement::View);
        items::set_requirement(&org.pool, scope(org.id), item, view, now())
            .await
            .expect("set requirement");
        Call {
            params: vec![("item_id", item)],
            body: Some(serde_json::json!({ "action": "view" })),
            at: At::Course(course),
        }
    })
}

/// Whether the path parameter `name` names an item or what an item points at.
fn names_content(name: &str) -> bool {
    CONTENT_PARAMS
        .iter()
        .any(|param| name == *param || name.ends_with(&format!("_{param}")))
}

/// The operations of `doc` whose path names an item or its content.
fn content_operations(doc: &Value) -> Vec<Operation> {
    operations(doc)
        .expect("named operations")
        .into_iter()
        .filter(|op| {
            op.path
                .split('/')
                .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
                .any(names_content)
        })
        .collect()
}

/// What is wrong between `doc` and `table`: content operations without an entry, entries that
/// are no content operation, and entries listed twice.
fn lock_walk(doc: &Value, table: &[&str]) -> Vec<String> {
    let content = content_operations(doc);
    let mut problems = Vec::new();
    for op in &content {
        if !table.contains(&op.id.as_str()) {
            problems.push(format!(
                "{} ({} {}) names an item or its content and has no lock suite entry: add it \
                 to LOCK_TABLE in tests/lock_suite/mod.rs",
                op.id, op.method, op.path
            ));
        }
    }
    for (i, name) in table.iter().enumerate() {
        if !content.iter().any(|op| op.id == *name) {
            problems.push(format!(
                "{name} is listed but is no operation naming an item or its content"
            ));
        }
        if table[..i].contains(name) {
            problems.push(format!("{name} is listed twice"));
        }
    }
    problems
}

fn lock_table_ids() -> Vec<&'static str> {
    LOCK_TABLE.iter().map(|entry| entry.operation_id).collect()
}

#[test]
fn t13_every_content_operation_is_in_the_lock_table() {
    let problems = lock_walk(&document(), &lock_table_ids());
    assert!(problems.is_empty(), "{problems:#?}");
}

/// A route naming a content id that no entry lists fails the walk by name, and so do a stale
/// entry and a repeated one.
#[test]
fn t13_an_unlisted_content_route_fails_the_walk_by_name() {
    let mut doc = document();
    let paths = doc["paths"].as_object_mut().expect("paths");
    paths.insert(
        "/api/v1/quizzes/{quiz_attempt_id}/videos/{video_id}/start".into(),
        serde_json::json!({ "post": { "operationId": "startSomething" } }),
    );
    let mut table = lock_table_ids();
    let problems = lock_walk(&doc, &table);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].starts_with("startSomething (POST "),
        "{problems:#?}"
    );

    table.push("getItem");
    table.push("listModules");
    let problems = lock_walk(&document(), &table);
    assert_eq!(
        problems,
        [
            "getItem is listed twice",
            "listModules is listed but is no operation naming an item or its content",
        ]
    );
}

#[test]
fn content_params_match_by_name_or_suffix() {
    for name in ["item_id", "page_id", "course_file_id", "video_id"] {
        assert!(names_content(name), "{name}");
    }
    for name in [
        "module_id",
        "course_id",
        "version_id",
        "profile_idx",
        "fileid",
    ] {
        assert!(!names_content(name), "{name}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn t13_every_content_route_checks_the_lock_or_is_staff_only(pool: PgPool) {
    let world = World::new(pool).await;
    let app = router(world.state()).layer(Extension(ConnectInfo(CLIENT)));
    let operations = operations(&document()).expect("named operations");
    let mut failures = Vec::new();
    for entry in LOCK_TABLE {
        let op = operations
            .iter()
            .find(|op| op.id == entry.operation_id)
            .unwrap_or_else(|| panic!("{} is not in the document", entry.operation_id));
        let result = match entry.lock {
            Lock::Checked(setup) => checked(&world, &app, op, setup).await,
            Lock::Read(setup) => read(&world, &app, op, setup).await,
            Lock::Staff => staff(&world, &app, op).await,
        };
        if let Err(why) = result {
            failures.push(format!("{} ({} {}): {why}", op.id, op.method, op.path));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A Student whose item is locked gets 403 `item.locked` with reasons, and nothing changes.
async fn checked(world: &World, app: &Router, op: &Operation, setup: Setup) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let student = org.holder_of_builtin("student", call.at).await;
    let before = snapshot(&world.pool, org.id).await?;
    let answer = call_op(app, op, &call, org.id, Some(&student)).await?;
    answer.expect(StatusCode::FORBIDDEN, "item.locked")?;
    let body: Value = serde_json::from_str(&answer.body).map_err(|e| e.to_string())?;
    if body["reasons"].as_array().is_none_or(Vec::is_empty) {
        return Err(format!("no reasons: {body}"));
    }
    if snapshot(&world.pool, org.id).await? != before {
        return Err("the organization's rows changed".into());
    }
    Ok(())
}

/// The same Student reads the locked item: 200, its lock, and nothing it points at.
async fn read(world: &World, app: &Router, op: &Operation, setup: Setup) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let student = org.holder_of_builtin("student", call.at).await;
    let call = Call { body: None, ..call };
    let answer = call_op(app, op, &call, org.id, Some(&student)).await?;
    answer.expect_success()?;
    let body: Value = serde_json::from_str(&answer.body).map_err(|e| e.to_string())?;
    if body["lock"]["reasons"].as_array().is_none_or(Vec::is_empty) {
        return Err(format!("no lock: {body}"));
    }
    if answer.body.contains(LINK_URL) || body.get("ref_id").is_some() {
        return Err(format!("the locked item's content: {body}"));
    }
    Ok(())
}

/// The route's permission suite entry requires a [`STAFF`] permission the Student and Observer
/// roles lack, and a Student enrolled in every course of the organization gets 403
/// `permission.denied`.
async fn staff(world: &World, app: &Router, op: &Operation) -> Result<(), String> {
    let Access::Permission { key, setup, .. } = &entry_of(&op.id).access else {
        return Err("its permission suite entry is not a permission entry".into());
    };
    if !STAFF.contains(key) {
        return Err(format!(
            "it requires {key}, which is not a staff permission"
        ));
    }
    for learner in ["student", "observer"] {
        let role = roles::builtin(learner).expect("a built-in role");
        if role.permissions().contains(key) {
            return Err(format!("the built-in {learner} holds {key}"));
        }
    }
    let org = world.org().await;
    let call = setup(&org).await;
    let courses: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM courses WHERE org_id = $1")
        .bind(org.id)
        .fetch_all(&world.pool)
        .await
        .map_err(|e| e.to_string())?;
    let (student, user) = org.user().await;
    let role = roles::builtin_id(&world.pool, "student")
        .await
        .map_err(|e| e.to_string())?
        .ok_or("no built-in student")?;
    for course in courses {
        org.enroll(user, role, course).await;
    }
    let answer = call_op(app, op, &call, org.id, Some(&student)).await?;
    answer.expect(StatusCode::FORBIDDEN, "permission.denied")
}
