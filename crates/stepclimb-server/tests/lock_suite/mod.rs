// SPDX-License-Identifier: AGPL-3.0-or-later
//! The lock suite (docs/specs/m1/04-unlock-and-completion-rules.md B13, T13), run with the
//! permission suite so it shares its walker, its setups and its organizations.
//!
//! Two kinds of operation of the OpenAPI document must have an entry in [`LOCK_TABLE`]; the walker
//! test fails, naming each, on one without:
//!
//! - every operation whose path names a module, an item or what an item points at (a path
//!   parameter in [`CONTENT_PARAMS`], or ending in `_` and one of them);
//! - every operation whose success answer (any 2xx) holds an `Item` or a `Module` (at any depth)
//!   and that a learner can call: its permission suite entry is not a permission entry, or
//!   names a permission the built-in Student or Observer holds, or it has no entry. However a
//!   later route is keyed (by course, by module, by a body), it cannot hand a learner an item
//!   without passing the read check below.
//!
//! From each entry:
//!
//! - **checked** (a route that hands a learner an item's content or accepts progress for it): a
//!   Student whose item is locked gets 403 `item.locked` with its reasons, and the
//!   organization's rows are unchanged;
//! - **read** (a route that answers a learner modules or items): a Student and an Observer get
//!   200 in which every module and item is locked and no item has `url` or `ref_id`, and the
//!   raw body holds neither the locked link's URL nor any stored `ref_id` (a page item's);
//! - **staff** (a route only staff call): its permission suite entry requires a permission in
//!   [`STAFF`], which neither the built-in Student nor the Observer holds, and a Student and an
//!   Observer enrolled in the target's course each get 403 `permission.denied`.

use super::*;

/// The path parameters that name a module, an item or what an item points at. Later content specs add
/// their ids (a quiz's, an activity's) here and their routes to [`LOCK_TABLE`].
const CONTENT_PARAMS: &[&str] = &["module_id", "item_id", "page_id", "file_id", "video_id"];

/// The permissions a staff-only content route may require: authors see an item's content
/// whatever its lock (B5).
const STAFF: &[&str] = &[CourseViewAll::KEY, CourseContentEdit::KEY];

/// How a content route keeps a locked item's content from a learner.
enum Lock {
    /// It checks the lock: 403 `item.locked`. The setup builds a published item, locked for a
    /// Student, and the request; the Student is enrolled at the call's `at`.
    Checked(Setup),
    /// It answers a learner modules or items: locked ones with B12's fields only. The setup
    /// builds a course whose only module is locked, holding a link and a page item, and names
    /// the course, the module and the page item.
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
        operation_id: "listModules",
        lock: Lock::Read(locked_outline),
    },
    LockEntry {
        operation_id: "getModule",
        lock: Lock::Read(locked_outline),
    },
    LockEntry {
        operation_id: "getItem",
        lock: Lock::Read(locked_outline),
    },
    LockEntry {
        operation_id: "updateModule",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "deleteModule",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "createItem",
        lock: Lock::Staff,
    },
    LockEntry {
        operation_id: "reorderItems",
        lock: Lock::Staff,
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
    LockEntry {
        operation_id: "retryVideo",
        lock: Lock::Staff,
    },
];

/// The URL of the suite's links ([`Org::link`]).
const LINK_URL: &str = "https://example.com/suite";

/// A published course in Tower whose only published module opens in a day, holding a
/// published link with a `view` requirement: the link is locked by its module. Its course,
/// module and link.
async fn locked_module(org: &Org) -> (Uuid, Uuid, Uuid) {
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
    (course, module, item)
}

/// [`locked_module`]'s link; the request views it.
fn view_locked_link(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (course, _, item) = locked_module(org).await;
        Call {
            params: vec![("item_id", item)],
            body: Some(serde_json::json!({ "action": "view" })),
            at: At::Course(course),
        }
    })
}

/// [`locked_module`] with a published page item after the link, pointing at a new id: the
/// course, the module and the page item, for whichever the read's path names.
fn locked_outline(org: &Org) -> Pin<Box<dyn Future<Output = Call> + Send + '_>> {
    Box::pin(async {
        let (course, module, _) = locked_module(org).await;
        let page_item = page_item(org, course, module).await;
        Call {
            params: vec![
                ("course_id", course),
                ("module_id", module),
                ("item_id", page_item),
            ],
            body: None,
            at: At::Course(course),
        }
    })
}

/// A published page item, last in `module`, whose `ref_id` is a new id, with a `view`
/// requirement. Written by SQL: no page item can be created through the API yet (M1 spec 03
/// B5), and the read check needs a `ref_id` a read could leak.
async fn page_item(org: &Org, course: Uuid, module: Uuid) -> Uuid {
    let id = new_id();
    sqlx::query(
        "INSERT INTO module_items
             (id, org_id, course_id, module_id, position, kind, ref_id, title, published,
              requirement)
         SELECT $1, $2, $3, $4, count(*), 'page', $5, 'Reading', true, '{\"kind\": \"view\"}'
         FROM module_items WHERE module_id = $4",
    )
    .bind(id)
    .bind(org.id)
    .bind(course)
    .bind(module)
    .bind(new_id())
    .execute(&org.pool)
    .await
    .expect("insert a page item");
    id
}

/// Whether the path parameter `name` names an item or what an item points at.
fn names_content(name: &str) -> bool {
    CONTENT_PARAMS
        .iter()
        .any(|param| name == *param || name.ends_with(&format!("_{param}")))
}

/// Whether `op`'s path names a module, an item or its content.
fn names_content_in_path(op: &Operation) -> bool {
    op.path
        .split('/')
        .filter_map(|segment| segment.strip_prefix('{')?.strip_suffix('}'))
        .any(names_content)
}

/// The schema names under `#/components/schemas/` that `value` reaches through `$ref`s, at any
/// depth, each once.
fn schemas_reached(doc: &Value, value: &Value, seen: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(name) = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| r.strip_prefix("#/components/schemas/"))
                && !seen.iter().any(|s| s == name)
            {
                seen.push(name.to_owned());
                schemas_reached(doc, &doc["components"]["schemas"][name], seen);
            }
            for member in map.values() {
                schemas_reached(doc, member, seen);
            }
        }
        Value::Array(values) => {
            for member in values {
                schemas_reached(doc, member, seen);
            }
        }
        _ => {}
    }
}

/// Whether a success answer (any 2xx) of `op` holds an `Item` or a `Module`.
fn answers_items(doc: &Value, op: &Operation) -> bool {
    let responses = &doc["paths"][&op.path][op.method.as_str().to_lowercase()]["responses"];
    let mut seen = Vec::new();
    for (status, answer) in responses.as_object().into_iter().flatten() {
        if status.starts_with('2') {
            schemas_reached(doc, answer, &mut seen);
        }
    }
    seen.iter().any(|name| name == "Item" || name == "Module")
}

/// Whether a learner (the built-in Student or Observer) can call `op`: anything but a
/// permission suite entry whose permission neither role holds. An operation without an entry
/// counts as callable.
fn learner_callable(op: &Operation) -> bool {
    let Some(entry) = TABLE.iter().find(|entry| entry.operation_id == op.id) else {
        return true;
    };
    let Access::Permission { key, .. } = &entry.access else {
        return true;
    };
    ["student", "observer"].iter().any(|role| {
        roles::builtin(role)
            .expect("a built-in role")
            .permissions()
            .contains(key)
    })
}

/// The operations of `doc` the lock suite must list: those whose path names a module, an item
/// or its content, and those a learner can call whose success answer holds an item or a module.
fn content_operations(doc: &Value) -> Vec<Operation> {
    operations(doc)
        .expect("named operations")
        .into_iter()
        .filter(|op| names_content_in_path(op) || (answers_items(doc, op) && learner_callable(op)))
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
                "{} ({} {}) names or answers a module, an item or its content and has no lock \
                 suite entry: add it to LOCK_TABLE in tests/lock_suite/mod.rs",
                op.id, op.method, op.path
            ));
        }
    }
    for (i, name) in table.iter().enumerate() {
        if !content.iter().any(|op| op.id == *name) {
            problems.push(format!(
                "{name} is listed but neither names nor answers a module, an item or its content"
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
    table.push("getCourse");
    let problems = lock_walk(&document(), &table);
    assert_eq!(
        problems,
        [
            "getItem is listed twice",
            "getCourse is listed but neither names nor answers a module, an item or its content",
        ]
    );
}

/// A route a learner can call that answers modules or items, keyed by nothing the path walk
/// knows (here a course), fails the walk by name too.
#[test]
fn t13_an_unlisted_route_answering_items_fails_the_walk_by_name() {
    let mut doc = document();
    let paths = doc["paths"].as_object_mut().expect("paths");
    let answer = serde_json::json!({ "content": { "application/json": {
        "schema": { "$ref": "#/components/schemas/ModuleList" } } } });
    paths.insert(
        "/api/v1/courses/{course_id}/syllabus".into(),
        serde_json::json!({ "get": { "operationId": "getSyllabus",
            "responses": { "200": answer } } }),
    );
    let problems = lock_walk(&doc, &lock_table_ids());
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].starts_with("getSyllabus (GET "),
        "{problems:#?}"
    );
}

/// The three outline reads are found by what they answer and who can call them; an author's
/// write that answers an item is not a learner's.
#[test]
fn learner_reads_are_found_by_their_answer_and_their_permission() {
    let doc = document();
    let ops = operations(&doc).expect("named operations");
    let op = |id: &str| ops.iter().find(|op| op.id == id).expect("an operation");
    for id in ["listModules", "getModule", "getItem"] {
        assert!(
            answers_items(&doc, op(id)) && learner_callable(op(id)),
            "{id}"
        );
    }
    for id in ["createItem", "updateItem"] {
        assert!(
            answers_items(&doc, op(id)) && !learner_callable(op(id)),
            "{id}"
        );
    }
    assert!(!answers_items(&doc, op("getCourse")));
}

#[test]
fn content_params_match_by_name_or_suffix() {
    for name in [
        "module_id",
        "item_id",
        "page_id",
        "course_file_id",
        "video_id",
    ] {
        assert!(names_content(name), "{name}");
    }
    for name in ["course_id", "version_id", "profile_idx", "fileid"] {
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

/// A Student and an Observer of the course read the locked module and its items: 200, every
/// module and item locked, no `url` or `ref_id` on any item, and neither the link's URL nor any
/// stored `ref_id` anywhere in the raw body.
async fn read(world: &World, app: &Router, op: &Operation, setup: Setup) -> Result<(), String> {
    let org = world.org().await;
    let call = setup(&org).await;
    let refs: Vec<Uuid> = sqlx::query_scalar(
        "SELECT ref_id FROM module_items WHERE org_id = $1 AND ref_id IS NOT NULL",
    )
    .bind(org.id)
    .fetch_all(&world.pool)
    .await
    .map_err(|e| e.to_string())?;
    if refs.is_empty() {
        return Err("the setup stored no ref_id, so the check could not fail".into());
    }
    for role in ["student", "observer"] {
        let cookie = org.holder_of_builtin(role, call.at).await;
        let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
        answer.expect_success()?;
        let leaked = answer.body.contains(LINK_URL)
            || refs.iter().any(|id| answer.body.contains(&id.to_string()));
        if leaked {
            return Err(format!("{role}: the locked content: {}", answer.body));
        }
        let body: Value = serde_json::from_str(&answer.body).map_err(|e| e.to_string())?;
        let mut rows = 0;
        check_rows(&body, &mut rows).map_err(|why| format!("{role}: {why}: {body}"))?;
        if rows == 0 {
            return Err(format!("{role}: no module or item in the answer: {body}"));
        }
    }
    Ok(())
}

/// Every module (an object with `items`) and item (an object with `requirement`) in `value` is
/// locked, and no item has `url` or `ref_id`; `rows` counts them.
fn check_rows(value: &Value, rows: &mut usize) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if map.contains_key("items") || map.contains_key("requirement") {
                *rows += 1;
                if map["lock"]["reasons"].as_array().is_none_or(Vec::is_empty) {
                    return Err(format!("a row without its lock: {}", map["id"]));
                }
                if map.contains_key("url") || map.contains_key("ref_id") {
                    return Err(format!("a locked item's content: {}", map["id"]));
                }
            }
            map.values().try_for_each(|member| check_rows(member, rows))
        }
        Value::Array(values) => values
            .iter()
            .try_for_each(|member| check_rows(member, rows)),
        _ => Ok(()),
    }
}

/// The route's permission suite entry requires a [`STAFF`] permission the Student and Observer
/// roles lack, and a Student and an Observer, each enrolled in every course of the
/// organization, get 403 `permission.denied`.
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
    for learner in ["student", "observer"] {
        let (cookie, user) = org.user().await;
        let role = roles::builtin_id(&world.pool, learner)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("no such built-in role")?;
        for &course in &courses {
            org.enroll(user, role, course).await;
        }
        let answer = call_op(app, op, &call, org.id, Some(&cookie)).await?;
        answer
            .expect(StatusCode::FORBIDDEN, "permission.denied")
            .map_err(|why| format!("{learner}: {why}"))?;
    }
    Ok(())
}
