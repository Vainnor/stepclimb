// SPDX-License-Identifier: AGPL-3.0-or-later
//! The accounts repository and the `accounts` table against a fresh database per test
//! (docs/specs/m0/02-organizations-and-accounts.md B16 to B22; T11 to T14 at the repository
//! level). Needs `DATABASE_URL` (`just up`).
//!
//! Organizations are made with `organizations::create`, so each has its root account. Every test
//! that writes through the repository checks that org B's tree is unchanged.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{PgConnection, PgPool, Postgres, pool::PoolConnection};
use stepclimb_core::{
    OrgScope,
    accounts::{self, Account, Error, MAX_DEPTH, MAX_NAME_CHARS, Reach},
    id::{Uuid, new_id},
    organizations,
};

/// 2025-11-13T02:13:20.123456Z: an odd microsecond count, so a rounded or truncated write shows.
fn request_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(1_763_000_000_123_456)
}

/// One second after [`request_time`].
fn later_time() -> SystemTime {
    request_time() + Duration::from_secs(1)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the credential resolvers do not exist yet; tests stand in for one"
)]
fn scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// A new organization and the id of its root account.
async fn org(pool: &PgPool, slug: &str) -> (Uuid, Uuid) {
    let org = organizations::create(pool, slug, slug, request_time())
        .await
        .expect("create organization");
    let root =
        sqlx::query_scalar("SELECT id FROM accounts WHERE org_id = $1 AND parent_id IS NULL")
            .bind(org.id)
            .fetch_one(pool)
            .await
            .expect("the root account");
    (org.id, root)
}

async fn create(pool: &PgPool, org: Uuid, parent: Uuid, name: &str) -> Account {
    accounts::create(pool, scope(org), parent, name, request_time())
        .await
        .expect("create account")
}

/// The stored path of an account, as text.
async fn path(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT path::text FROM accounts WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read path")
}

fn hex(id: Uuid) -> String {
    id.simple().to_string()
}

/// Every account of the organization, every column as text, in id order: a snapshot that shows
/// whether a call changed anything.
async fn tree(pool: &PgPool, org: Uuid) -> Vec<String> {
    sqlx::query_scalar("SELECT to_jsonb(a)::text FROM accounts a WHERE org_id = $1 ORDER BY id")
        .bind(org)
        .fetch_all(pool)
        .await
        .expect("read tree")
}

/// Accounts of the organization whose path is not their parent's path plus their own id (or,
/// for the root, its id alone). Empty for a consistent tree.
async fn inconsistent_paths(pool: &PgPool, org: Uuid) -> Vec<Uuid> {
    sqlx::query_scalar(
        "SELECT a.id FROM accounts a
         LEFT JOIN accounts p ON p.org_id = a.org_id AND p.id = a.parent_id
         WHERE a.org_id = $1
           AND a.path <> COALESCE(p.path, ''::ltree) || replace(a.id::text, '-', '')::ltree",
    )
    .bind(org)
    .fetch_all(pool)
    .await
    .expect("check paths")
}

/// The variant name of an accounts error, for comparing in a table of cases.
fn kind<T: std::fmt::Debug>(result: &Result<T, Error>) -> String {
    match result {
        Ok(value) => format!("Ok({value:?})"),
        Err(error) => format!("{error:?}")
            .split([' ', '(', '{'])
            .next()
            .unwrap_or_default()
            .to_owned(),
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_builds_the_path_from_the_parents_and_reads_back(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;

    let tower = create(&pool, a, root, "  Tower ").await;
    let ground = create(&pool, a, tower.id, "Ground").await;

    // T11: root.tower.ground, as hex ids.
    assert_eq!(
        path(&pool, ground.id).await,
        format!("{}.{}.{}", hex(root), hex(tower.id), hex(ground.id))
    );
    assert_eq!(tower.name, "Tower", "the name is trimmed");
    assert_eq!(tower.parent_id, Some(root));
    assert_eq!(tower.depth, 1);
    assert!(!tower.has_children, "Tower had no child when it was made");
    assert_eq!(ground.depth, 2);
    assert_eq!(ground.id.get_version_num(), 7);
    assert_eq!(ground.created_at, request_time());
    assert_eq!(ground.updated_at, request_time());

    let tower_now = accounts::get(&pool, scope(a), tower.id).await.unwrap();
    assert!(tower_now.as_ref().unwrap().has_children);
    let root_read = accounts::get(&pool, scope(a), root).await.unwrap().unwrap();
    assert_eq!(
        (
            root_read.parent_id,
            root_read.depth,
            root_read.name.as_str()
        ),
        (None, 0, "org-a")
    );
    assert_eq!(
        accounts::get(&pool, scope(a), ground.id).await.unwrap(),
        Some(ground)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_finds_an_account_of_the_scope_only(pool: PgPool) {
    let (a, _) = org(&pool, "org-a").await;
    let (b, root_b) = org(&pool, "org-b").await;
    let tower_b = create(&pool, b, root_b, "Tower").await;

    assert_eq!(
        accounts::get(&pool, scope(a), tower_b.id).await.unwrap(),
        None
    );
    assert_eq!(accounts::get(&pool, scope(a), root_b).await.unwrap(), None);
    assert_eq!(
        accounts::get(&pool, scope(a), new_id()).await.unwrap(),
        None
    );
    assert_eq!(
        accounts::get(&pool, scope(b), tower_b.id).await.unwrap(),
        Some(tower_b)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_a_parent_outside_the_scope_and_inserts_nothing(pool: PgPool) {
    let (a, _) = org(&pool, "org-a").await;
    let (b, root_b) = org(&pool, "org-b").await;
    let a_before = tree(&pool, a).await;
    let b_before = tree(&pool, b).await;

    let other_org = accounts::create(&pool, scope(a), root_b, "Tower", request_time()).await;
    let missing = accounts::create(&pool, scope(a), new_id(), "Tower", request_time()).await;

    assert!(
        matches!(other_org, Err(Error::ParentNotFound)),
        "{other_org:?}"
    );
    assert!(matches!(missing, Err(Error::ParentNotFound)), "{missing:?}");
    assert_eq!(tree(&pool, a).await, a_before);
    assert_eq!(tree(&pool, b).await, b_before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_and_rename_refuse_invalid_names_and_change_nothing(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;
    let tower = create(&pool, a, root, "Tower").await;
    let before = tree(&pool, a).await;
    let long = "é".repeat(MAX_NAME_CHARS + 1);
    let cases = [
        (" \t ", "NameEmpty"),
        (long.as_str(), "NameTooLong"),
        ("Tow\0er", "ContainsNul"),
    ];

    for (name, expected) in cases {
        let created = accounts::create(&pool, scope(a), root, name, request_time()).await;
        let renamed = accounts::rename(&pool, scope(a), tower.id, name, later_time()).await;
        assert_eq!(kind(&created), expected, "create {name:?}");
        assert_eq!(kind(&renamed), expected, "rename {name:?}");
    }
    assert_eq!(tree(&pool, a).await, before);

    let at_limit = "é".repeat(MAX_NAME_CHARS);
    let renamed = accounts::rename(&pool, scope(a), tower.id, &at_limit, later_time()).await;
    assert_eq!(renamed.unwrap().name, at_limit);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn rename_changes_the_name_and_updated_at_only(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;
    let (b, _) = org(&pool, "org-b").await;
    let tower = create(&pool, a, root, "Tower").await;
    let ground = create(&pool, a, tower.id, "Ground").await;
    let paths_before = (path(&pool, tower.id).await, path(&pool, ground.id).await);
    let ground_before = accounts::get(&pool, scope(a), ground.id).await.unwrap();
    let b_before = tree(&pool, b).await;

    let renamed = accounts::rename(&pool, scope(a), tower.id, " Tower Cab ", later_time())
        .await
        .unwrap();

    // T11: renaming touches no path.
    assert_eq!(renamed.name, "Tower Cab");
    assert_eq!(renamed.updated_at, later_time());
    assert_eq!(renamed.created_at, request_time());
    assert_eq!(renamed.parent_id, Some(root));
    assert!(renamed.has_children);
    assert_eq!(
        (path(&pool, tower.id).await, path(&pool, ground.id).await),
        paths_before
    );
    assert_eq!(
        accounts::get(&pool, scope(a), ground.id).await.unwrap(),
        ground_before
    );
    assert_eq!(tree(&pool, b).await, b_before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn writes_with_another_organizations_scope_are_not_found_and_change_nothing(pool: PgPool) {
    let (a, root_a) = org(&pool, "org-a").await;
    let (b, root_b) = org(&pool, "org-b").await;
    let tower_b = create(&pool, b, root_b, "Tower").await;
    let enroute_b = create(&pool, b, root_b, "Enroute").await;
    let enroute_a = create(&pool, a, root_a, "Enroute").await;
    let b_before = tree(&pool, b).await;
    let a_before = tree(&pool, a).await;
    let now = later_time();

    let rename = accounts::rename(&pool, scope(a), tower_b.id, "Taken", now).await;
    let moved = accounts::move_to(&pool, scope(a), tower_b.id, enroute_b.id, now).await;
    let into_b = accounts::move_to(&pool, scope(a), enroute_a.id, root_b, now).await;
    let deleted = accounts::delete(&pool, scope(a), tower_b.id).await;

    assert_eq!(kind(&rename), "NotFound");
    assert_eq!(kind(&moved), "NotFound");
    assert_eq!(kind(&into_b), "ParentNotFound");
    assert_eq!(kind(&deleted), "NotFound");
    assert_eq!(tree(&pool, b).await, b_before);
    assert_eq!(tree(&pool, a).await, a_before);
}

/// Root, then Tower with Ground below it and Ramp below Ground, and Enroute beside Tower.
struct Facility {
    org: Uuid,
    root: Uuid,
    tower: Uuid,
    ground: Uuid,
    ramp: Uuid,
    enroute: Uuid,
}

async fn facility(pool: &PgPool, slug: &str) -> Facility {
    let (org, root) = org(pool, slug).await;
    let tower = create(pool, org, root, "Tower").await.id;
    let ground = create(pool, org, tower, "Ground").await.id;
    let ramp = create(pool, org, ground, "Ramp").await.id;
    let enroute = create(pool, org, root, "Enroute").await.id;
    Facility {
        org,
        root,
        tower,
        ground,
        ramp,
        enroute,
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn move_rewrites_the_path_of_the_account_and_every_descendant(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let other = facility(&pool, "org-b").await;
    let other_before = tree(&pool, other.org).await;
    let root_before = accounts::get(&pool, scope(f.org), f.root).await.unwrap();

    let moved = accounts::move_to(&pool, scope(f.org), f.tower, f.enroute, later_time())
        .await
        .unwrap();

    // T12.
    assert_eq!(moved.parent_id, Some(f.enroute));
    assert_eq!(moved.depth, 2);
    assert_eq!(moved.updated_at, later_time());
    let expected = [
        (f.tower, vec![f.root, f.enroute, f.tower]),
        (f.ground, vec![f.root, f.enroute, f.tower, f.ground]),
        (f.ramp, vec![f.root, f.enroute, f.tower, f.ground, f.ramp]),
    ];
    for (id, chain) in expected {
        let want: Vec<String> = chain.into_iter().map(hex).collect();
        assert_eq!(path(&pool, id).await, want.join("."));
    }
    assert_eq!(inconsistent_paths(&pool, f.org).await, Vec::<Uuid>::new());
    let ground = accounts::get(&pool, scope(f.org), f.ground)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        ground.parent_id,
        Some(f.tower),
        "a descendant keeps its parent"
    );
    assert_eq!(ground.depth, 3);
    assert_eq!(
        ground.updated_at,
        later_time(),
        "its depth changed with the move"
    );
    assert_eq!(
        accounts::get(&pool, scope(f.org), f.root).await.unwrap(),
        root_before,
        "an account outside the moved subtree is untouched"
    );
    assert_eq!(tree(&pool, other.org).await, other_before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn move_refuses_the_root_and_moves_into_the_own_subtree(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let before = tree(&pool, f.org).await;
    let now = later_time();

    // T13.
    let root = accounts::move_to(&pool, scope(f.org), f.root, f.enroute, now).await;
    let under_child = accounts::move_to(&pool, scope(f.org), f.tower, f.ground, now).await;
    let under_grandchild = accounts::move_to(&pool, scope(f.org), f.tower, f.ramp, now).await;
    let under_itself = accounts::move_to(&pool, scope(f.org), f.tower, f.tower, now).await;
    let missing_parent = accounts::move_to(&pool, scope(f.org), f.tower, new_id(), now).await;
    let missing = accounts::move_to(&pool, scope(f.org), new_id(), f.root, now).await;

    assert_eq!(kind(&root), "RootImmutable");
    assert_eq!(kind(&under_child), "MoveIntoDescendant");
    assert_eq!(kind(&under_grandchild), "MoveIntoDescendant");
    assert_eq!(kind(&under_itself), "MoveIntoDescendant");
    assert_eq!(kind(&missing_parent), "ParentNotFound");
    assert_eq!(kind(&missing), "NotFound");
    assert_eq!(tree(&pool, f.org).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn move_to_a_sibling_branch_and_back_keeps_the_tree_consistent(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let now = later_time();

    accounts::move_to(&pool, scope(f.org), f.ground, f.enroute, now)
        .await
        .unwrap();
    accounts::move_to(&pool, scope(f.org), f.enroute, f.tower, now)
        .await
        .unwrap();
    accounts::move_to(&pool, scope(f.org), f.ground, f.root, now)
        .await
        .unwrap();

    assert_eq!(inconsistent_paths(&pool, f.org).await, Vec::<Uuid>::new());
    assert_eq!(
        path(&pool, f.ramp).await,
        format!("{}.{}.{}", hex(f.root), hex(f.ground), hex(f.ramp))
    );
}

/// A connection with an open transaction, for holding the tree lock while another call waits.
async fn begin(pool: &PgPool) -> sqlx::Transaction<'static, Postgres> {
    pool.begin().await.expect("begin")
}

/// The tree lock's keys as `pg_locks` shows them (`classid`, `objid`, as unsigned 32-bit
/// values): a copy of the repository's private `TREE_LOCK_CLASS` and key fold. If the two drift,
/// [`until_a_call_waits_on_the_tree_lock`] times out and the test fails.
fn tree_lock_keys(org: Uuid) -> (i64, i64) {
    let class: i32 = 0x6163_6374;
    let (words, _) = org.as_bytes().as_chunks::<4>();
    let key = words
        .iter()
        .map(|word| i32::from_be_bytes(*word))
        .fold(0, |key, word| key ^ word);
    (
        i64::from(class.cast_unsigned()),
        i64::from(key.cast_unsigned()),
    )
}

/// Waits until a session of this test's database waits on `org`'s tree lock, so the test knows
/// the second call has read nothing yet. Matches the lock exactly: sqlx's test harness takes
/// advisory locks of its own, in another database. Each poll borrows a pool connection only for
/// its query.
async fn until_a_call_waits_on_the_tree_lock(pool: &PgPool, org: Uuid) {
    let (class, key) = tree_lock_keys(org);
    for _ in 0..500 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_locks
             WHERE locktype = 'advisory' AND NOT granted
               AND database = (SELECT oid FROM pg_database WHERE datname = current_database())
               AND classid = $1::bigint::oid AND objid = $2::bigint::oid AND objsubid = 2",
        )
        .bind(class)
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("read pg_locks");
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no session waited on the tree lock within 5 s");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn crossing_moves_in_two_open_transactions_end_with_one_refused_and_no_cycle(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    // Both transactions are open before either move runs: two pool connections, held until each
    // commits.
    let mut first = begin(&pool).await;
    let mut second = begin(&pool).await;
    accounts::move_to(&mut *first, scope(f.org), f.tower, f.enroute, later_time())
        .await
        .unwrap();
    // The crossing move runs in the second transaction while the first is uncommitted.
    let crossing = tokio::spawn({
        let (org, enroute, tower) = (f.org, f.enroute, f.tower);
        async move {
            let result =
                accounts::move_to(&mut *second, scope(org), enroute, tower, later_time()).await;
            second
                .commit()
                .await
                .expect("commit the second transaction");
            result
        }
    });
    until_a_call_waits_on_the_tree_lock(&pool, f.org).await;
    first.commit().await.unwrap();

    let crossing = crossing.await.expect("the second move ran");

    // T12: the second move read the tree only after the first committed.
    assert_eq!(kind(&crossing), "MoveIntoDescendant");
    assert_eq!(inconsistent_paths(&pool, f.org).await, Vec::<Uuid>::new());
    let tower = accounts::get(&pool, scope(f.org), f.tower)
        .await
        .unwrap()
        .unwrap();
    let enroute = accounts::get(&pool, scope(f.org), f.enroute)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tower.parent_id, Some(f.enroute));
    assert_eq!(enroute.parent_id, Some(f.root), "no cycle");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn tree_writes_refuse_an_isolation_level_above_read_committed(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let before = tree(&pool, f.org).await;
    let mut conn = pool.acquire().await.unwrap();

    let levels = [
        (
            "repeatable read",
            "SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        ),
        (
            "serializable",
            "SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL SERIALIZABLE",
        ),
    ];

    for (level, set_level) in levels {
        sqlx::raw_sql(set_level).execute(&mut *conn).await.unwrap();

        let moved =
            accounts::move_to(&mut *conn, scope(f.org), f.tower, f.enroute, later_time()).await;
        let created = accounts::create(&mut *conn, scope(f.org), f.root, "New", later_time()).await;
        let deleted = accounts::delete(&mut *conn, scope(f.org), f.ramp).await;

        for result in [kind(&moved), kind(&created), kind(&deleted)] {
            assert_eq!(result, "IsolationLevel", "{level}");
        }
        assert!(
            matches!(&moved, Err(Error::IsolationLevel { found }) if found == level),
            "{moved:?}"
        );
    }
    assert_eq!(tree(&pool, f.org).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_move_to_the_current_parent_changes_nothing(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let before = tree(&pool, f.org).await;
    let tower = accounts::get(&pool, scope(f.org), f.tower).await.unwrap();

    let moved = accounts::move_to(&pool, scope(f.org), f.tower, f.root, later_time())
        .await
        .unwrap();

    assert_eq!(Some(moved), tower, "updated_at keeps its old value");
    assert_eq!(tree(&pool, f.org).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn delete_refuses_the_root_and_an_account_with_children(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let before = tree(&pool, f.org).await;

    // T14.
    let root = accounts::delete(&pool, scope(f.org), f.root).await;
    let with_child = accounts::delete(&pool, scope(f.org), f.ground).await;
    let missing = accounts::delete(&pool, scope(f.org), new_id()).await;

    assert_eq!(kind(&root), "RootImmutable");
    assert_eq!(kind(&with_child), "HasChildren");
    assert_eq!(kind(&missing), "NotFound");
    assert_eq!(tree(&pool, f.org).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn delete_removes_a_leaf_only(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let other = facility(&pool, "org-b").await;
    let other_before = tree(&pool, other.org).await;

    accounts::delete(&pool, scope(f.org), f.ramp).await.unwrap();

    assert_eq!(
        accounts::get(&pool, scope(f.org), f.ramp).await.unwrap(),
        None
    );
    let ground = accounts::get(&pool, scope(f.org), f.ground)
        .await
        .unwrap()
        .unwrap();
    assert!(!ground.has_children);
    assert_eq!(tree(&pool, f.org).await.len(), 4);
    assert_eq!(tree(&pool, other.org).await, other_before);
    accounts::delete(&pool, scope(f.org), f.ground)
        .await
        .expect("Ground is a leaf now");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_create_waiting_on_a_delete_of_its_parent_is_refused(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let mut deleting = begin(&pool).await;
    accounts::delete(&mut *deleting, scope(f.org), f.ramp)
        .await
        .unwrap();
    let creating = tokio::spawn({
        let pool = pool.clone();
        let (org, ramp) = (f.org, f.ramp);
        async move { accounts::create(&pool, scope(org), ramp, "Apron", later_time()).await }
    });
    until_a_call_waits_on_the_tree_lock(&pool, f.org).await;
    deleting.commit().await.unwrap();

    let created = creating.await.expect("the create ran");

    assert_eq!(kind(&created), "ParentNotFound");
}

/// A chain of accounts below the root, `levels` deep; returns their ids, shallowest first.
async fn chain(pool: &PgPool, org: Uuid, root: Uuid, levels: i32) -> Vec<Uuid> {
    let mut ids = Vec::new();
    let mut parent = root;
    for level in 1..=levels {
        parent = create(pool, org, parent, &format!("Level {level}"))
            .await
            .id;
        ids.push(parent);
    }
    ids
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_an_account_deeper_than_the_limit(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;
    let ids = chain(&pool, a, root, MAX_DEPTH).await;
    let deepest = *ids.last().unwrap();
    let deepest_read = accounts::get(&pool, scope(a), deepest)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(deepest_read.depth, MAX_DEPTH, "depth 16 is allowed");
    let before = tree(&pool, a).await;

    let too_deep = accounts::create(&pool, scope(a), deepest, "Too deep", request_time()).await;

    assert_eq!(kind(&too_deep), "TooDeep");
    assert_eq!(tree(&pool, a).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn move_refuses_a_subtree_that_would_end_deeper_than_the_limit(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;
    let long = chain(&pool, a, root, MAX_DEPTH - 2).await;
    // A two-level subtree: Branch with Leaf below it.
    let branch = create(&pool, a, root, "Branch").await.id;
    create(&pool, a, branch, "Leaf").await;
    let before = tree(&pool, a).await;

    // Under the account at depth 14, Branch lands at 15 and Leaf at 16: allowed.
    let fits = long[long.len() - 1];
    // A sibling account at depth 15 would put Leaf at 17.
    let deeper = create(&pool, a, fits, "Deeper").await.id;
    let too_deep = accounts::move_to(&pool, scope(a), branch, deeper, later_time()).await;
    assert_eq!(kind(&too_deep), "TooDeep");
    accounts::delete(&pool, scope(a), deeper).await.unwrap();
    assert_eq!(tree(&pool, a).await, before);

    let moved = accounts::move_to(&pool, scope(a), branch, fits, later_time())
        .await
        .unwrap();
    assert_eq!(moved.depth, MAX_DEPTH - 1);
    assert_eq!(inconsistent_paths(&pool, a).await, Vec::<Uuid>::new());
}

/// Names of a list result.
fn names(accounts: &[Account]) -> Vec<&str> {
    accounts.iter().map(|a| a.name.as_str()).collect()
}

/// Inserts an account under `parent` with a chosen id, so a test can store rows out of id order.
async fn insert_with_id(pool: &PgPool, org: Uuid, parent: Uuid, id: Uuid, name: &str) {
    sqlx::query(
        "INSERT INTO accounts (id, org_id, parent_id, name, path)
         SELECT $1, $2, $3, $4, path || $5::text::ltree FROM accounts WHERE id = $3",
    )
    .bind(id)
    .bind(org)
    .bind(parent)
    .bind(name)
    .bind(hex(id))
    .execute(pool)
    .await
    .expect("insert account");
}

/// A connection on which PostgreSQL may not read a table through an index, so ties come back
/// in heap order unless the query sorts them by id (see `tests/users.rs`, which explains why
/// this can only weaken the tie tests, never make them flake).
async fn without_index_scans(pool: &PgPool) -> PoolConnection<Postgres> {
    let mut conn = pool.acquire().await.expect("a connection");
    sqlx::query(
        "SELECT set_config('enable_indexscan', 'off', false),
                set_config('enable_indexonlyscan', 'off', false),
                set_config('enable_bitmapscan', 'off', false)",
    )
    .execute(&mut *conn)
    .await
    .expect("turn off index scans");
    conn
}

/// Every page of `parent`'s children, `limit` at a time, joined. Fails after `max_pages` pages.
async fn all_pages(
    conn: &mut PgConnection,
    org: Uuid,
    parent: Uuid,
    limit: i64,
    max_pages: usize,
) -> Vec<Account> {
    let mut joined: Vec<Account> = Vec::new();
    for _ in 0..max_pages {
        let after = joined.last().map(|a| (a.name.clone(), a.id));
        let page = accounts::list_children(&mut *conn, scope(org), parent, after.as_ref(), limit)
            .await
            .expect("list a page");
        let done = page.len() < usize::try_from(limit).expect("a positive limit");
        joined.extend(page);
        if done {
            return joined;
        }
    }
    panic!("limit {limit}: still paging after {max_pages} pages; a page repeated");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_children_sorts_by_name_then_id_and_pages_through_ties(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let other = facility(&pool, "org-b").await;
    // Four children named Approach, stored in descending id order.
    let mut ids: Vec<Uuid> = (0..4).map(|_| new_id()).collect();
    for id in ids.iter().rev() {
        insert_with_id(&pool, f.org, f.root, *id, "Approach").await;
    }
    ids.sort();

    let mut conn = without_index_scans(&pool).await;
    let whole = accounts::list_children(&mut *conn, scope(f.org), f.root, None, 100)
        .await
        .unwrap();

    let mut expected = vec!["Approach"; 4];
    expected.extend(["Enroute", "Tower"]);
    assert_eq!(
        names(&whole),
        expected,
        "only direct children of org A's root"
    );
    let tie: Vec<Uuid> = whole[..4].iter().map(|a| a.id).collect();
    assert_eq!(tie, ids, "equal names sort by id");
    let tower = whole.iter().find(|a| a.id == f.tower).unwrap();
    assert!(tower.has_children);
    assert_eq!(tower.depth, 1);
    for limit in [1, 3] {
        let joined = all_pages(&mut conn, f.org, f.root, limit, whole.len() + 1).await;
        assert_eq!(
            joined, whole,
            "limit {limit}: no account repeated or skipped"
        );
    }
    let from_a = accounts::list_children(&mut *conn, scope(f.org), other.root, None, 100).await;
    assert_eq!(
        from_a.unwrap(),
        Vec::new(),
        "org B's children are not listed for org A"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_top_visible_lists_each_reached_account_below_no_other(pool: PgPool) {
    let f = facility(&pool, "org-a").await;
    let other = facility(&pool, "org-b").await;
    let pool = &pool;
    let org = f.org;
    let top = async |reach: Reach<'_>| {
        accounts::list_top_visible(pool, scope(org), reach, None, 100).await
    };

    let whole = top(Reach::Organization).await.unwrap();
    assert_eq!(whole.iter().map(|a| a.id).collect::<Vec<_>>(), [f.root]);
    assert_eq!(whole[0].depth, 0);

    // T16: Tower and Enroute, by name; Tower and Ground (its child) give Tower only; Ground and
    // Ramp (its child) give Ground, even though Tower above them is not reached.
    let pairs = [
        (vec![f.tower, f.enroute], vec!["Enroute", "Tower"]),
        (vec![f.ground, f.tower], vec!["Tower"]),
        (vec![f.ramp, f.ground], vec!["Ground"]),
        (vec![f.root, f.ramp], vec!["org-a"]),
        (vec![], Vec::<&str>::new()),
    ];
    for (ids, expected) in pairs {
        let listed = top(Reach::Accounts(&ids)).await.unwrap();
        assert_eq!(names(&listed), expected, "{ids:?}");
    }

    // Another organization's ids reach nothing, and do not hide org A's accounts below them.
    let foreign = [other.root, other.tower, f.ground];
    let listed = top(Reach::Accounts(&foreign)).await.unwrap();
    assert_eq!(names(&listed), ["Ground"]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_top_visible_pages_by_name_then_id(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;
    let mut ids = Vec::new();
    for name in ["Bravo", "Alpha", "Bravo", "Charlie"] {
        ids.push(create(&pool, a, root, name).await.id);
    }
    let reach = Reach::Accounts(&ids);
    let whole = accounts::list_top_visible(&pool, scope(a), reach, None, 100)
        .await
        .unwrap();
    assert_eq!(names(&whole), ["Alpha", "Bravo", "Bravo", "Charlie"]);
    assert!(whole[1].id < whole[2].id, "equal names sort by id");
    let after = (whole[1].name.clone(), whole[1].id);
    let rest = accounts::list_top_visible(&pool, scope(a), reach, Some(&after), 2)
        .await
        .unwrap();
    assert_eq!(rest, whole[2..4]);
    let nul = ("A\0".to_owned(), new_id());
    let refused = accounts::list_top_visible(&pool, scope(a), reach, Some(&nul), 2).await;
    assert!(matches!(
        refused,
        Err(Error::ContainsNul { field: "cursor" })
    ));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_children_refuses_nul_in_the_cursor(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;
    let key = ("Tower\0".to_owned(), new_id());

    let result = accounts::list_children(&pool, scope(a), root, Some(&key), 10).await;

    assert!(
        matches!(result, Err(Error::ContainsNul { field: "cursor" })),
        "{result:?}"
    );
}

/// The SQLSTATE and constraint of a refused statement.
fn refusal(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> (String, Option<String>) {
    let error = result.expect_err("the statement is refused");
    let db = error.as_database_error().expect("a database error");
    (
        db.code().map(|c| c.into_owned()).unwrap_or_default(),
        db.constraint().map(str::to_owned),
    )
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_keeps_a_parent_in_the_childs_organization(pool: PgPool) {
    let (a, _) = org(&pool, "org-a").await;
    let (_, root_b) = org(&pool, "org-b").await;
    let id = new_id();

    let across = sqlx::query(
        "INSERT INTO accounts (id, org_id, parent_id, name, path)
         VALUES ($1, $2, $3, 'Across', $4::text::ltree)",
    )
    .bind(id)
    .bind(a)
    .bind(root_b)
    .bind(format!("{}.{}", hex(root_b), hex(id)))
    .execute(&pool)
    .await;

    assert_eq!(
        refusal(across),
        (
            "23503".to_owned(),
            Some("accounts_org_id_parent_id_fkey".to_owned())
        ),
        "foreign_key_violation on the composite key"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_table_ties_a_missing_parent_to_a_one_label_path(pool: PgPool) {
    let (a, root) = org(&pool, "org-a").await;
    let id = new_id();

    let child_with_short_path = sqlx::query(
        "INSERT INTO accounts (id, org_id, parent_id, name, path)
         VALUES ($1, $2, $3, 'Child', $4::text::ltree)",
    )
    .bind(id)
    .bind(a)
    .bind(root)
    .bind(hex(id))
    .execute(&pool)
    .await;
    let blank_name = sqlx::query("UPDATE accounts SET name = '  ' WHERE id = $1")
        .bind(root)
        .execute(&pool)
        .await;
    let delete_parent = sqlx::query("DELETE FROM accounts WHERE id = $1").bind(root);
    create(&pool, a, root, "Tower").await;
    let delete_parent = delete_parent.execute(&pool).await;

    assert_eq!(refusal(child_with_short_path).0, "23514", "check_violation");
    assert_eq!(refusal(blank_name).0, "23514", "check_violation");
    assert_eq!(
        refusal(delete_parent).0,
        "23503",
        "the foreign key (NO ACTION) refuses deleting a parent"
    );
}
