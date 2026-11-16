// SPDX-License-Identifier: AGPL-3.0-or-later
//! The course files repository and the cleanup queries of `files` against a fresh database per
//! test (docs/specs/m1/06-files.md B9 to B12, B21): names and folders, the folder listing, the
//! folder rename, the delete and the stamp it leaves, which rows cleanup may take, and that no
//! function reaches another organization's rows. Needs `DATABASE_URL` (`just up`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{PgConnection, PgPool};
use stepclimb_core::{
    OrgScope,
    audit::Actor,
    course_files::{self, CourseFile, Entry, Error, Folder, Name},
    courses::{self, NewCourse, Status},
    files::{self, NewFile, Sha256Hasher},
    id::{Uuid, new_id},
    organizations,
};

/// 2025-11-13T02:13:20Z.
fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

const HOUR: Duration = Duration::from_secs(60 * 60);

#[expect(
    clippy::disallowed_methods,
    reason = "tests stand in for the credential resolvers"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// An organization with one course: their ids.
async fn org_with_course(pool: &PgPool, slug: &str) -> (Uuid, Uuid) {
    let org = organizations::create(pool, slug, slug, now())
        .await
        .expect("create organization")
        .id;
    (org, course(pool, org, "C1").await)
}

async fn course(pool: &PgPool, org: Uuid, code: &str) -> Uuid {
    let root = sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1")
        .bind(org)
        .fetch_one(pool)
        .await
        .expect("the root");
    let new = NewCourse {
        account_id: root,
        title: format!("Course {code}"),
        code: code.to_owned(),
    };
    courses::create(pool, scope(org), &new, now(), Actor::Instance)
        .await
        .expect("create course")
        .id
}

/// A stored file of `org` holding `bytes`: its id.
async fn stored(pool: &PgPool, org: Uuid, bytes: &[u8]) -> Uuid {
    let mut hasher = Sha256Hasher::new();
    hasher.update(bytes);
    let new = NewFile {
        hashed: hasher.finish(),
        mime: "text/plain; charset=utf-8".into(),
    };
    files::insert(pool, scope(org), &new, now())
        .await
        .expect("insert file")
        .id
}

async fn conn(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    pool.acquire().await.expect("a connection")
}

async fn place(
    pool: &PgPool,
    org: Uuid,
    course: Uuid,
    file: Uuid,
    at: (&str, &str),
) -> Result<CourseFile, Error> {
    let folder = Folder::parse(at.0).expect("a folder");
    let name = Name::parse(at.1).expect("a name");
    let mut conn = conn(pool).await;
    course_files::create(&mut conn, scope(org), course, file, (&folder, &name), now()).await
}

async fn paths(pool: &PgPool, course: Uuid) -> Vec<(String, String)> {
    sqlx::query_as("SELECT folder, name FROM course_files WHERE course_id = $1 ORDER BY 1, 2")
        .bind(course)
        .fetch_all(pool)
        .await
        .expect("read course files")
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_placed_file_reads_back_with_its_bytes_details(pool: PgPool) {
    let (org, course) = org_with_course(&pool, "org-a").await;
    let file = stored(&pool, org, b"notes").await;
    let placed = place(&pool, org, course, file, ("/week-1", "Cafe\u{301}.txt"))
        .await
        .unwrap();
    assert_eq!(placed.name, "Caf\u{e9}.txt");
    assert_eq!((placed.folder.as_str(), placed.size), ("/week-1", 5));
    assert_eq!(placed.mime, "text/plain; charset=utf-8");
    assert_eq!(placed.file_id, file);
    let read = course_files::get(&pool, scope(org), placed.id)
        .await
        .unwrap();
    assert_eq!(read, Some(placed.clone()));
    let of = course_files::course_of(&pool, scope(org), placed.id).await;
    assert_eq!(of.unwrap(), Some(course));
    // The composed and the decomposed name are the same name.
    let again = place(&pool, org, course, file, ("/week-1", "Café.txt")).await;
    assert!(matches!(again, Err(Error::NameTaken)), "{again:?}");
    let (folder, name) = (
        Folder::parse("/week-1").unwrap(),
        Name::parse("Café.txt").unwrap(),
    );
    let taken = course_files::name_taken(&pool, scope(org), course, (&folder, &name)).await;
    assert!(taken.unwrap());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_archived_course_takes_no_file(pool: PgPool) {
    let (org, course) = org_with_course(&pool, "org-a").await;
    let file = stored(&pool, org, b"x").await;
    let placed = place(&pool, org, course, file, ("/", "a.txt"))
        .await
        .unwrap();
    for status in [Status::Published, Status::Archived] {
        courses::set_status(&pool, scope(org), course, status, now(), Actor::Instance)
            .await
            .expect("change status");
    }
    let refused = place(&pool, org, course, file, ("/", "b.txt")).await;
    assert!(matches!(refused, Err(Error::CourseArchived)), "{refused:?}");
    let mut conn = conn(&pool).await;
    let name = Name::parse("c.txt").unwrap();
    let rename = course_files::update(&mut conn, scope(org), placed.id, (None, Some(&name)), now());
    assert!(matches!(rename.await, Err(Error::CourseArchived)));
    let delete = course_files::delete(&mut conn, scope(org), placed.id, now()).await;
    assert!(matches!(delete, Err(Error::CourseArchived)), "{delete:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_listing_has_folders_then_files_by_name_page_by_page(pool: PgPool) {
    let (org, course) = org_with_course(&pool, "org-a").await;
    let file = stored(&pool, org, b"x").await;
    for at in [
        ("/", "b.txt"),
        ("/", "A.txt"),
        ("/zeta", "1"),
        ("/alpha/deep", "2"),
        ("/alpha", "3"),
        ("/alphabet", "4"),
    ] {
        place(&pool, org, course, file, at).await.unwrap();
    }
    let list = |folder: &str, after: Option<course_files::ListKey>, limit| {
        let (pool, folder) = (pool.clone(), Folder::parse(folder).unwrap());
        async move {
            course_files::list_folder(&pool, scope(org), course, &folder, after.as_ref(), limit)
                .await
                .unwrap()
        }
    };
    let names = |entries: &[Entry]| -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                Entry::Folder { name } => format!("{name}/"),
                Entry::File(file) => file.name.clone(),
            })
            .collect()
    };
    let all = list("/", None, 50).await;
    // Bytes order: "A" before "b"; folders first.
    assert_eq!(
        names(&all),
        ["alpha/", "alphabet/", "zeta/", "A.txt", "b.txt"]
    );
    let first = list("/", None, 2).await;
    assert_eq!(names(&first), ["alpha/", "alphabet/"]);
    let rest = list("/", Some(first[1].key()), 50).await;
    assert_eq!(names(&rest), ["zeta/", "A.txt", "b.txt"]);
    let after_folders = list("/", Some((0, "zzz".into())), 50).await;
    assert_eq!(names(&after_folders), ["A.txt", "b.txt"]);
    assert_eq!(names(&list("/alpha", None, 50).await), ["deep/", "3"]);
    assert_eq!(names(&list("/alpha/deep", None, 50).await), ["2"]);
    assert!(list("/missing", None, 50).await.is_empty());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_folder_rename_moves_its_whole_subtree_or_nothing(pool: PgPool) {
    let (org, course) = org_with_course(&pool, "org-a").await;
    let file = stored(&pool, org, b"x").await;
    for at in [("/a", "x"), ("/a/c", "y"), ("/ab", "z")] {
        place(&pool, org, course, file, at).await.unwrap();
    }
    let rename = |from: &str, to: &str| {
        let (pool, from, to) = (
            pool.clone(),
            Folder::parse(from).unwrap(),
            Folder::parse(to).unwrap(),
        );
        async move {
            let mut conn = conn(&pool).await;
            course_files::rename_folder(&mut conn, scope(org), course, (&from, &to), now()).await
        }
    };
    assert_eq!(rename("/a", "/b").await.unwrap(), 2);
    let moved = [("/ab", "z"), ("/b", "x"), ("/b/c", "y")].map(|(f, n)| (f.into(), n.into()));
    assert_eq!(paths(&pool, course).await, moved);
    // Renaming /b to /d meets the existing /d/x: nothing moves.
    place(&pool, org, course, file, ("/d", "x")).await.unwrap();
    let before = paths(&pool, course).await;
    let mut tx = pool.begin().await.unwrap();
    let (from, to) = (Folder::parse("/b").unwrap(), Folder::parse("/d").unwrap());
    let collided =
        course_files::rename_folder(&mut tx, scope(org), course, (&from, &to), now()).await;
    assert!(matches!(collided, Err(Error::NameTaken)), "{collided:?}");
    drop(tx);
    assert_eq!(paths(&pool, course).await, before);
    let deep = rename("/b", "/1/2/3/4/5/6/7/8").await;
    assert!(matches!(deep, Err(Error::TooDeep)), "{deep:?}");
    assert!(matches!(
        rename("/nothing", "/x").await,
        Err(Error::FolderNotFound)
    ));
    assert!(matches!(rename("/", "/x").await, Err(Error::RootFolder)));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_or_rename_changes_only_what_it_names(pool: PgPool) {
    let (org, course) = org_with_course(&pool, "org-a").await;
    let file = stored(&pool, org, b"x").await;
    let placed = place(&pool, org, course, file, ("/a", "x.txt"))
        .await
        .unwrap();
    place(&pool, org, course, file, ("/b", "x.txt"))
        .await
        .unwrap();
    let mut conn = conn(&pool).await;
    let later = now() + HOUR;
    let unchanged = course_files::update(&mut conn, scope(org), placed.id, (None, None), later)
        .await
        .unwrap();
    assert_eq!(unchanged.updated_at, now());
    let to = Folder::parse("/b").unwrap();
    let taken =
        course_files::update(&mut conn, scope(org), placed.id, (Some(&to), None), later).await;
    assert!(matches!(taken, Err(Error::NameTaken)), "{taken:?}");
    let name = Name::parse("y.txt").unwrap();
    let moved = course_files::update(
        &mut conn,
        scope(org),
        placed.id,
        (Some(&to), Some(&name)),
        later,
    )
    .await
    .unwrap();
    assert_eq!(
        (moved.folder.as_str(), moved.name.as_str()),
        ("/b", "y.txt")
    );
    assert_eq!(moved.updated_at, later);
}

/// A `file` item pointing at `course_file`, by SQL: `file` items become creatable with spec 06 I3.
async fn file_item(pool: &PgPool, org: Uuid, course: Uuid, course_file: Uuid) {
    let module = stepclimb_core::modules::create(pool, scope(org), course, "M", now())
        .await
        .expect("create module")
        .id;
    sqlx::query(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, ref_id, title)
         VALUES ($1, $2, $3, $4, 0, 'file', $5, 'Handout')",
    )
    .bind(new_id())
    .bind(org)
    .bind(course)
    .bind(module)
    .bind(course_file)
    .execute(pool)
    .await
    .expect("insert file item");
}

async fn delete(pool: &PgPool, org: Uuid, id: Uuid, at: SystemTime) -> Result<(), Error> {
    course_files::delete(&mut *conn(pool).await, scope(org), id, at).await
}

async fn unreferenced(pool: &PgPool, org: Uuid, at: SystemTime) -> Vec<Uuid> {
    let cutoff = at - 24 * HOUR;
    let rows = files::unreferenced(pool, scope(org), cutoff)
        .await
        .expect("unreferenced");
    rows.into_iter().map(|(id, _)| id).collect()
}

async fn delete_file(pool: &PgPool, org: Uuid, id: Uuid, at: SystemTime) -> bool {
    let mut conn: PgConnection = conn(pool).await.detach();
    files::delete(&mut conn, scope(org), id, at - 24 * HOUR)
        .await
        .expect("delete")
        .is_some()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_file_is_unreferenced_24_hours_after_its_last_course_file_goes(pool: PgPool) {
    let (org, course) = org_with_course(&pool, "org-a").await;
    let other_course = course_of(&pool, org).await;
    let shared = stored(&pool, org, b"shared").await;
    let alone = stored(&pool, org, b"alone").await;
    let in_a = place(&pool, org, course, shared, ("/", "s.txt"))
        .await
        .unwrap();
    place(&pool, org, other_course, shared, ("/", "s.txt"))
        .await
        .unwrap();
    let lone = place(&pool, org, course, alone, ("/", "a.txt"))
        .await
        .unwrap();
    // A file item uses the course file: it cannot be deleted.
    let used = place(&pool, org, course, alone, ("/", "used.txt"))
        .await
        .unwrap();
    file_item(&pool, org, course, used.id).await;
    assert!(matches!(
        delete(&pool, org, used.id, now()).await,
        Err(Error::InUse)
    ));
    let t0 = now() + HOUR;
    delete(&pool, org, in_a.id, t0).await.unwrap();
    delete(&pool, org, lone.id, t0).await.unwrap();
    // `alone` is still used by the item's course file; `shared` by the other course.
    assert_eq!(
        unreferenced(&pool, org, t0 + 25 * HOUR).await,
        Vec::<Uuid>::new()
    );
    sqlx::query("DELETE FROM module_items")
        .execute(&pool)
        .await
        .unwrap();
    delete(&pool, org, used.id, t0 + HOUR).await.unwrap();
    assert_eq!(
        unreferenced(&pool, org, t0 + 24 * HOUR).await,
        Vec::<Uuid>::new()
    );
    assert_eq!(unreferenced(&pool, org, t0 + 25 * HOUR).await, [alone]);
    assert!(!delete_file(&pool, org, alone, t0 + 24 * HOUR).await);
    assert!(!delete_file(&pool, org, shared, t0 + 48 * HOUR).await);
    assert!(delete_file(&pool, org, alone, t0 + 25 * HOUR).await);
    assert!(
        !files::exists(&pool, scope(org), sha(b"alone"))
            .await
            .unwrap()
    );
    assert!(
        files::exists(&pool, scope(org), sha(b"shared"))
            .await
            .unwrap()
    );
}

async fn course_of(pool: &PgPool, org: Uuid) -> Uuid {
    course(
        pool,
        org,
        &format!("C{}", &new_id().simple().to_string()[..8]),
    )
    .await
}

fn sha(bytes: &[u8]) -> files::Sha256Digest {
    let mut hasher = Sha256Hasher::new();
    hasher.update(bytes);
    hasher.finish().sha256()
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_file_never_placed_is_unreferenced_from_its_creation(pool: PgPool) {
    let (org, _) = org_with_course(&pool, "org-a").await;
    let file = stored(&pool, org, b"orphan row").await;
    assert_eq!(
        unreferenced(&pool, org, now() + 23 * HOUR).await,
        Vec::<Uuid>::new()
    );
    assert_eq!(unreferenced(&pool, org, now() + 24 * HOUR).await, [file]);
    assert!(files::any_of_size(&pool, scope(org), 10).await.unwrap());
    assert!(!files::any_of_size(&pool, scope(org), 11).await.unwrap());
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn no_function_reaches_another_organizations_course_files(pool: PgPool) {
    let (a, course_a) = org_with_course(&pool, "org-a").await;
    let (b, _) = org_with_course(&pool, "org-b").await;
    let file = stored(&pool, a, b"theirs").await;
    let placed = place(&pool, a, course_a, file, ("/x", "t.txt"))
        .await
        .unwrap();
    place(&pool, a, course_a, file, ("/", "top.txt"))
        .await
        .unwrap();
    assert_eq!(
        course_files::get(&pool, scope(b), placed.id).await.unwrap(),
        None
    );
    assert_eq!(
        course_files::course_of(&pool, scope(b), placed.id)
            .await
            .unwrap(),
        None
    );
    let root = Folder::root();
    let listed = course_files::list_folder(&pool, scope(b), course_a, &root, None, 50).await;
    assert!(listed.unwrap().is_empty());
    assert!(matches!(
        delete(&pool, b, placed.id, now()).await,
        Err(Error::NotFound)
    ));
    let mut conn = conn(&pool).await;
    let name = Name::parse("mine.txt").unwrap();
    let rename = course_files::update(&mut conn, scope(b), placed.id, (None, Some(&name)), now());
    assert!(matches!(rename.await, Err(Error::NotFound)));
    let (from, to) = (Folder::parse("/x").unwrap(), Folder::parse("/y").unwrap());
    let moved =
        course_files::rename_folder(&mut conn, scope(b), course_a, (&from, &to), now()).await;
    assert!(matches!(moved, Err(Error::CourseNotFound)), "{moved:?}");
    // Org B cannot place org A's file in its own course either: the foreign key keeps both in
    // one organization.
    let course_b = course_of(&pool, b).await;
    let crossed = place(&pool, b, course_b, file, ("/", "x.txt")).await;
    assert!(matches!(crossed, Err(Error::Database(_))), "{crossed:?}");
    assert!(
        files::unreferenced(&pool, scope(b), now() + 48 * HOUR)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !files::exists(&pool, scope(b), sha(b"theirs"))
            .await
            .unwrap()
    );
    assert!(
        files::delete(&mut conn, scope(b), file, now() + 48 * HOUR)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        paths(&pool, course_a).await,
        [
            ("/".to_owned(), "top.txt".to_owned()),
            ("/x".to_owned(), "t.txt".to_owned())
        ]
    );
}

/// An upload's transaction lock on a hash and cleanup's session lock on it take turns (B7,
/// B21); another hash or another organization's same hash does not wait.
#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_hash_lock_makes_an_upload_and_cleanup_take_turns(pool: PgPool) {
    let (a, _) = org_with_course(&pool, "org-a").await;
    let (b, _) = org_with_course(&pool, "org-b").await;
    let (hash, other) = (sha(b"same"), sha(b"other"));
    let mut upload = pool.begin().await.unwrap();
    files::lock_hash(&mut upload, scope(a), hash).await.unwrap();
    // A session that waits more than 300 ms for a lock gets an error instead.
    let mut cleanup = conn(&pool).await;
    sqlx::query("SET lock_timeout = '300ms'")
        .execute(&mut *cleanup)
        .await
        .unwrap();
    let blocked = files::lock_hash_session(&mut cleanup, scope(a), hash).await;
    assert!(blocked.is_err(), "the session lock did not wait");
    for (org, digest) in [(a, other), (b, hash)] {
        files::lock_hash_session(&mut cleanup, scope(org), digest)
            .await
            .expect("no wait");
        let released = files::unlock_hash_session(&mut cleanup, scope(org), digest).await;
        assert!(released.unwrap());
    }
    upload.commit().await.unwrap();
    files::lock_hash_session(&mut cleanup, scope(a), hash)
        .await
        .expect("the lock is free after the commit");
    assert!(
        files::unlock_hash_session(&mut cleanup, scope(a), hash)
            .await
            .unwrap()
    );
    assert!(
        !files::unlock_hash_session(&mut cleanup, scope(a), hash)
            .await
            .unwrap()
    );
}
