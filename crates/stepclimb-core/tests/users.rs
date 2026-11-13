// SPDX-License-Identifier: AGPL-3.0-or-later
//! The users repository and the `users` table against a fresh database per test
//! (docs/specs/m0/03-users-and-identities.md B1 to B8, B12; T1, T3, T4, T8). Needs
//! `DATABASE_URL` (`just up`).
//!
//! Every test that reads or writes through the repository makes two organizations and checks
//! that a call with org A's scope reads or changes org A only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::{PgConnection, PgPool, Postgres, pool::PoolConnection};
use stepclimb_core::{
    OrgScope, Source,
    audit::Actor,
    id::{Uuid, new_id},
    users::{self, Error, ListFilter, NewUser, Status, User, UserPatch},
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

async fn insert_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("insert organization");
    id
}

fn new_user(display_name: &str) -> NewUser {
    NewUser {
        display_name: display_name.into(),
        given_name: String::new(),
        family_name: String::new(),
        email: None,
        locale: None,
        tz: "UTC".into(),
        source: Source::Local,
        external_id: None,
    }
}

async fn create(pool: &PgPool, org: Uuid, user: NewUser) -> User {
    users::create(pool, scope(org), &user, request_time())
        .await
        .expect("create user")
}

/// Every column of a `users` row as text, to show a refused or other-organization call left it
/// unchanged.
async fn stored(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT to_jsonb(u)::text FROM users u WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read user")
}

/// The SQLSTATE of a failed statement.
fn sqlstate(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> String {
    let error = result.expect_err("the statement is refused");
    error
        .as_database_error()
        .and_then(|e| e.code())
        .map(|c| c.into_owned())
        .unwrap_or_else(|| panic!("not a database error: {error}"))
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn get_finds_a_user_of_the_scope_only(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user_b = create(&pool, b, new_user("Sam Ortiz")).await;

    let from_a = users::get(&pool, scope(a), user_b.id).await.unwrap();
    let from_b = users::get(&pool, scope(b), user_b.id).await.unwrap();

    assert_eq!(from_a, None, "org A's scope does not see org B's user (T1)");
    assert_eq!(from_b, Some(user_b));
    assert_eq!(users::get(&pool, scope(b), new_id()).await.unwrap(), None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_stores_the_defaults_and_the_clocks_time(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;

    let user = create(&pool, a, new_user("  Sam Ortiz  ")).await;

    assert_eq!(
        user.display_name, "Sam Ortiz",
        "the display name is trimmed"
    );
    assert_eq!(user.given_name, "", "an omitted given name is ''");
    assert_eq!(user.family_name, "");
    assert_eq!(user.email, None);
    assert_eq!(user.locale, "en", "the locale defaults to en (B3)");
    assert_eq!(user.tz, "UTC");
    assert_eq!(user.status, Status::Active);
    assert_eq!(user.source, Source::Local);
    assert_eq!(user.external_id, None);
    assert_eq!(user.created_at, request_time());
    assert_eq!(user.updated_at, request_time());
    assert_eq!(
        user.id.get_version_num(),
        7,
        "the server makes a UUID v7 key"
    );
    assert_eq!(
        users::get(&pool, scope(a), user.id).await.unwrap(),
        Some(user)
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_stores_every_field_it_is_given(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let given = NewUser {
        display_name: "Sam Ortiz".into(),
        given_name: " Sam ".into(),
        family_name: "Ortiz".into(),
        email: Some("  Sam@Example.test ".into()),
        locale: Some("en".into()),
        tz: "America/New_York".into(),
        source: Source::Idp,
        external_id: Some("E1".into()),
    };

    let user = create(&pool, a, given).await;

    assert_eq!(
        user.given_name, " Sam ",
        "given and family names are stored as given"
    );
    assert_eq!(user.family_name, "Ortiz");
    assert_eq!(
        user.email.as_deref(),
        Some("Sam@Example.test"),
        "the email is trimmed, its case kept (B12)"
    );
    assert_eq!(user.tz, "America/New_York");
    assert_eq!(user.source, Source::Idp);
    assert_eq!(user.external_id.as_deref(), Some("E1"));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_invalid_fields_and_inserts_nothing(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let now = request_time();
    let blank_name = new_user(" \t ");
    let blank_email = NewUser {
        email: Some("  ".into()),
        ..new_user("Sam")
    };
    let french = NewUser {
        locale: Some("fr".into()),
        ..new_user("Sam")
    };

    let blank_name = users::create(&pool, scope(a), &blank_name, now).await;
    let blank_email = users::create(&pool, scope(a), &blank_email, now).await;
    let french = users::create(&pool, scope(a), &french, now).await;

    assert!(
        matches!(blank_name, Err(Error::DisplayNameEmpty)),
        "{blank_name:?}"
    );
    assert!(
        matches!(blank_email, Err(Error::EmailEmpty)),
        "{blank_email:?}"
    );
    assert!(
        matches!(french, Err(Error::LocaleUnsupported)),
        "{french:?}"
    );
    assert_eq!(users::count_for_limit(&pool, scope(a)).await.unwrap(), 0);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_external_id_is_unique_within_an_organization_only(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let with_e1 = |name: &str| NewUser {
        external_id: Some("E1".into()),
        source: Source::Sync,
        ..new_user(name)
    };
    create(&pool, a, with_e1("First")).await;

    let second_in_a = users::create(&pool, scope(a), &with_e1("Second"), request_time()).await;
    let in_b = users::create(&pool, scope(b), &with_e1("Other org"), request_time()).await;

    assert!(
        matches!(second_in_a, Err(Error::ExternalIdTaken)),
        "{second_in_a:?} (T4)"
    );
    assert_eq!(in_b.unwrap().external_id.as_deref(), Some("E1"));
    // Users without an external id never collide.
    create(&pool, a, new_user("No id 1")).await;
    create(&pool, a, new_user("No id 2")).await;
    assert_eq!(users::count_for_limit(&pool, scope(a)).await.unwrap(), 3);
}

fn with_email(name: &str, email: &str) -> NewUser {
    NewUser {
        email: Some(email.into()),
        ..new_user(name)
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_refuses_an_email_another_user_of_the_organization_has_in_any_case(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    create(&pool, a, with_email("First", "sam@example.test")).await;
    let now = request_time();

    let same = users::create(
        &pool,
        scope(a),
        &with_email("Same", "sam@example.test"),
        now,
    )
    .await;
    let other_case = users::create(
        &pool,
        scope(a),
        &with_email("Case", " SAM@Example.TEST "),
        now,
    )
    .await;

    assert!(matches!(same, Err(Error::EmailTaken)), "{same:?}");
    assert!(
        matches!(other_case, Err(Error::EmailTaken)),
        "B12: {other_case:?}"
    );
    assert_eq!(users::count_for_limit(&pool, scope(a)).await.unwrap(), 1);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn two_organizations_may_each_have_a_user_with_the_same_email(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    create(&pool, a, with_email("In A", "sam@example.test")).await;

    let in_b = users::create(
        &pool,
        scope(b),
        &with_email("In B", "SAM@example.test"),
        request_time(),
    )
    .await;

    assert_eq!(in_b.unwrap().email.as_deref(), Some("SAM@example.test"));
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn users_without_an_email_never_collide(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;

    create(&pool, a, new_user("First")).await;
    create(&pool, a, new_user("Second")).await;

    assert_eq!(users::count_for_limit(&pool, scope(a)).await.unwrap(), 2);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_refuses_an_email_another_user_of_the_organization_has(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    create(&pool, a, with_email("First", "sam@example.test")).await;
    let second = create(&pool, a, with_email("Second", "blair@example.test")).await;
    let before = stored(&pool, second.id).await;
    let patch = UserPatch {
        email: Some(Some("Sam@Example.test".into())),
        ..UserPatch::default()
    };

    let result = users::update(&pool, scope(a), second.id, &patch, later_time()).await;

    assert!(matches!(result, Err(Error::EmailTaken)), "{result:?}");
    assert_eq!(stored(&pool, second.id).await, before);
    let own_in_new_case = UserPatch {
        email: Some(Some("BLAIR@example.test".into())),
        ..UserPatch::default()
    };
    let recased = users::update(&pool, scope(a), second.id, &own_in_new_case, later_time()).await;
    assert_eq!(
        recased.unwrap().email.as_deref(),
        Some("BLAIR@example.test"),
        "a user may change the case of their own address"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_changes_the_named_fields_of_one_user(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let given = NewUser {
        given_name: "Sam".into(),
        email: Some("sam@example.test".into()),
        ..new_user("Sam Ortiz")
    };
    let user = create(&pool, a, given).await;
    let other = create(&pool, a, new_user("Other")).await;
    let other_before = stored(&pool, other.id).await;
    let patch = UserPatch {
        display_name: Some("  Samantha Ortiz ".into()),
        family_name: Some("Ortiz".into()),
        tz: Some("Europe/Berlin".into()),
        ..UserPatch::default()
    };

    let updated = users::update(&pool, scope(a), user.id, &patch, later_time())
        .await
        .unwrap();

    assert_eq!(updated.display_name, "Samantha Ortiz");
    assert_eq!(
        updated.given_name, "Sam",
        "a field the patch does not name is kept"
    );
    assert_eq!(updated.family_name, "Ortiz");
    assert_eq!(updated.email.as_deref(), Some("sam@example.test"));
    assert_eq!(updated.tz, "Europe/Berlin");
    assert_eq!(updated.created_at, request_time());
    assert_eq!(
        updated.updated_at,
        later_time(),
        "updated_at is the clock's reading"
    );
    assert_eq!(
        users::get(&pool, scope(a), user.id).await.unwrap(),
        Some(updated)
    );
    assert_eq!(stored(&pool, other.id).await, other_before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_sets_and_clears_the_email(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let user = create(&pool, a, new_user("Sam")).await;
    let set = UserPatch {
        email: Some(Some(" sam@example.test ".into())),
        ..UserPatch::default()
    };
    let clear = UserPatch {
        email: Some(None),
        ..UserPatch::default()
    };

    let after_set = users::update(&pool, scope(a), user.id, &set, later_time()).await;
    assert_eq!(
        after_set.unwrap().email.as_deref(),
        Some("sam@example.test")
    );
    let after_clear = users::update(&pool, scope(a), user.id, &clear, later_time()).await;
    assert_eq!(after_clear.unwrap().email, None);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn update_refuses_invalid_fields_and_changes_nothing(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let user = create(&pool, a, new_user("Sam")).await;
    let before = stored(&pool, user.id).await;
    let patches = [
        (
            UserPatch {
                display_name: Some("  ".into()),
                ..UserPatch::default()
            },
            Error::DisplayNameEmpty,
        ),
        (
            UserPatch {
                email: Some(Some(" ".into())),
                ..UserPatch::default()
            },
            Error::EmailEmpty,
        ),
        (
            UserPatch {
                locale: Some("fr".into()),
                ..UserPatch::default()
            },
            Error::LocaleUnsupported,
        ),
    ];

    for (patch, expected) in patches {
        let result = users::update(&pool, scope(a), user.id, &patch, later_time()).await;
        let error = result.expect_err("the patch is refused");
        assert_eq!(
            std::mem::discriminant(&error),
            std::mem::discriminant(&expected),
            "{patch:?}: {error:?}"
        );
        assert_eq!(stored(&pool, user.id).await, before, "{patch:?}");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn writes_with_another_organizations_scope_are_not_found_and_change_nothing(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user_b = create(&pool, b, new_user("Sam")).await;
    let before = stored(&pool, user_b.id).await;
    let patch = UserPatch {
        display_name: Some("Taken over".into()),
        ..UserPatch::default()
    };

    let update = users::update(&pool, scope(a), user_b.id, &patch, later_time()).await;
    let suspend = users::set_status(
        &pool,
        scope(a),
        user_b.id,
        Status::Suspended,
        later_time(),
        Actor::Instance,
    )
    .await;

    assert!(matches!(update, Err(Error::NotFound)), "{update:?}");
    assert!(matches!(suspend, Err(Error::NotFound)), "{suspend:?}");
    assert_eq!(stored(&pool, user_b.id).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn set_status_suspends_and_restores_a_user(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let user = create(&pool, a, new_user("Sam")).await;

    let suspended = users::set_status(
        &pool,
        scope(a),
        user.id,
        Status::Suspended,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(suspended.status, Status::Suspended);
    assert_eq!(suspended.updated_at, later_time());
    let read = users::get(&pool, scope(a), user.id).await.unwrap().unwrap();
    assert_eq!(read.status, Status::Suspended);

    let restored = users::set_status(
        &pool,
        scope(a),
        user.id,
        Status::Active,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    assert_eq!(restored.status, Status::Active);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn writes_refuse_a_time_the_database_cannot_store(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let before_epoch = UNIX_EPOCH - Duration::from_secs(1);

    let created = users::create(&pool, scope(a), &new_user("Sam"), before_epoch).await;

    assert!(matches!(created, Err(Error::Time(_))), "{created:?}");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn count_for_limit_counts_every_user_of_the_scope_only(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    assert_eq!(users::count_for_limit(&pool, scope(a)).await.unwrap(), 0);
    let first = create(&pool, a, new_user("First")).await;
    create(&pool, a, new_user("Second")).await;
    create(&pool, b, new_user("Other org")).await;
    users::set_status(
        &pool,
        scope(a),
        first.id,
        Status::Suspended,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();

    assert_eq!(
        users::count_for_limit(&pool, scope(a)).await.unwrap(),
        2,
        "suspended users count, other organizations' users do not"
    );
    assert_eq!(users::count_for_limit(&pool, scope(b)).await.unwrap(), 1);
}

/// Display names of `list`'s result.
fn names(users: &[User]) -> Vec<&str> {
    users.iter().map(|u| u.display_name.as_str()).collect()
}

/// Inserts a user with a chosen id, so a test can store rows out of id order.
async fn insert_user_with_id(pool: &PgPool, org: Uuid, id: Uuid, name: &str) {
    sqlx::query(
        "INSERT INTO users (id, org_id, source, display_name, tz) VALUES ($1, $2, 'local', $3, 'UTC')",
    )
    .bind(id)
    .bind(org)
    .bind(name)
    .execute(pool)
    .await
    .expect("insert user");
}

/// A connection on which PostgreSQL may not read a table through an index. The `users_by_name`
/// index already holds rows in `(display_name, id)` order, so through it a query returns ties in
/// id order even without `id` in its `ORDER BY`; without it the rows come from the heap, which
/// the tie tests fill in descending id order. A future PostgreSQL that ignored these settings or
/// read the heap in another order could only make the tests stop catching a missing `id`
/// tiebreak, never flake: with `id` in the `ORDER BY` the result is fully determined.
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

/// Every page of an unfiltered list of the scope's users, `limit` at a time, joined. Fails after
/// `max_pages` pages, so a cursor that hands back the same page again fails fast.
async fn all_pages(conn: &mut PgConnection, org: Uuid, limit: i64, max_pages: usize) -> Vec<User> {
    let mut joined: Vec<User> = Vec::new();
    for _ in 0..max_pages {
        let after = joined.last().map(|u| (u.display_name.clone(), u.id));
        let page = users::list(
            &mut *conn,
            scope(org),
            &ListFilter::default(),
            after.as_ref(),
            limit,
        )
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
async fn list_sorts_by_display_name_then_id(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    // Six users named Alex, stored in descending id order, so neither the order rows were
    // written in nor the order of their ids on disk hands back the tie in id order by accident.
    let mut alex_ids: Vec<Uuid> = (0..6).map(|_| new_id()).collect();
    for id in alex_ids.iter().rev() {
        insert_user_with_id(&pool, a, *id, "Alex").await;
    }
    for name in ["Casey", "Blair"] {
        create(&pool, a, new_user(name)).await;
    }
    create(&pool, b, new_user("Aaron")).await;

    let mut conn = without_index_scans(&pool).await;
    let all = users::list(&mut *conn, scope(a), &ListFilter::default(), None, 100)
        .await
        .unwrap();

    let mut expected = vec!["Alex"; 6];
    expected.extend(["Blair", "Casey"]);
    assert_eq!(names(&all), expected, "org B's user is absent");
    alex_ids.sort();
    let tie: Vec<Uuid> = all[..6].iter().map(|u| u.id).collect();
    assert_eq!(tie, alex_ids, "equal names sort by id");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn pages_split_inside_a_tie_join_into_the_whole_list(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let mut ids: Vec<Uuid> = (0..4).map(|_| new_id()).collect();
    for id in ids.iter().rev() {
        insert_user_with_id(&pool, a, *id, "Alex").await;
    }
    for name in ["Casey", "Blair"] {
        create(&pool, a, new_user(name)).await;
    }
    let mut conn = without_index_scans(&pool).await;
    let whole = users::list(&mut *conn, scope(a), &ListFilter::default(), None, 100)
        .await
        .unwrap();
    ids.sort();
    let whole_ids: Vec<Uuid> = whole.iter().map(|u| u.id).collect();
    assert_eq!(whole_ids[..4], ids[..], "the tie is in id order");

    // Limits 1 and 3 put a page boundary between two users named Alex.
    for limit in [1, 3] {
        let joined = all_pages(&mut conn, a, limit, whole.len() + 1).await;
        assert_eq!(joined, whole, "limit {limit}: no user repeated or skipped");
    }
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_refuses_nul_in_the_prefix_or_the_cursor(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let filter = ListFilter {
        q: Some("Sa\0".into()),
        ..ListFilter::default()
    };
    let key = ("Sam\0".to_owned(), new_id());

    let by_q = users::list(&pool, scope(a), &filter, None, 10).await;
    let by_cursor = users::list(&pool, scope(a), &ListFilter::default(), Some(&key), 10).await;

    assert!(
        matches!(by_q, Err(Error::ContainsNul { field: "q" })),
        "{by_q:?}"
    );
    assert!(
        matches!(by_cursor, Err(Error::ContainsNul { field: "cursor" })),
        "{by_cursor:?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn create_and_update_refuse_nul_in_every_string(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let now = request_time();
    let cases: [(&str, NewUser); 6] = [
        ("display_name", new_user("Sam\0")),
        (
            "given_name",
            NewUser {
                given_name: "Sam\0".into(),
                ..new_user("Sam")
            },
        ),
        (
            "family_name",
            NewUser {
                family_name: "O\0".into(),
                ..new_user("Sam")
            },
        ),
        ("email", with_email("Sam", "sam\0@example.test")),
        (
            "tz",
            NewUser {
                tz: "UTC\0".into(),
                ..new_user("Sam")
            },
        ),
        (
            "external_id",
            NewUser {
                external_id: Some("E\0".into()),
                ..new_user("Sam")
            },
        ),
    ];
    for (field, user) in cases {
        let result = users::create(&pool, scope(a), &user, now).await;
        assert!(
            matches!(result, Err(Error::ContainsNul { field: f }) if f == field),
            "{field}: {result:?}"
        );
    }
    assert_eq!(users::count_for_limit(&pool, scope(a)).await.unwrap(), 0);

    let user = create(&pool, a, new_user("Sam")).await;
    let before = stored(&pool, user.id).await;
    let patches: [(&str, UserPatch); 5] = [
        (
            "display_name",
            UserPatch {
                display_name: Some("S\0".into()),
                ..UserPatch::default()
            },
        ),
        (
            "given_name",
            UserPatch {
                given_name: Some("S\0".into()),
                ..UserPatch::default()
            },
        ),
        (
            "family_name",
            UserPatch {
                family_name: Some("O\0".into()),
                ..UserPatch::default()
            },
        ),
        (
            "email",
            UserPatch {
                email: Some(Some("s\0@example.test".into())),
                ..UserPatch::default()
            },
        ),
        (
            "tz",
            UserPatch {
                tz: Some("UTC\0".into()),
                ..UserPatch::default()
            },
        ),
    ];
    for (field, patch) in patches {
        let result = users::update(&pool, scope(a), user.id, &patch, later_time()).await;
        assert!(
            matches!(result, Err(Error::ContainsNul { field: f }) if f == field),
            "{field}: {result:?}"
        );
    }
    assert_eq!(stored(&pool, user.id).await, before);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn an_email_with_an_invisible_character_is_refused(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    create(&pool, a, with_email("Sam", "sam@example.test")).await;
    // Zero-width space: looks like the address above, would not collide with it.
    let lookalike = with_email("Lookalike", "sam\u{200B}@example.test");

    let created = users::create(&pool, scope(a), &lookalike, request_time()).await;

    assert!(
        matches!(created, Err(Error::EmailInvalidCharacter)),
        "{created:?}"
    );
    let user = create(&pool, a, new_user("Blair")).await;
    let patch = UserPatch {
        email: Some(Some("blair\u{FEFF}@example.test".into())),
        ..UserPatch::default()
    };
    let updated = users::update(&pool, scope(a), user.id, &patch, later_time()).await;
    assert!(
        matches!(updated, Err(Error::EmailInvalidCharacter)),
        "{updated:?}"
    );
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn list_filters_by_status_source_and_a_prefix(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let with = |name: &str, email: Option<&str>, source: Source| NewUser {
        email: email.map(Into::into),
        source,
        ..new_user(name)
    };
    create(&pool, a, with("Sam Ortiz", None, Source::Local)).await;
    create(
        &pool,
        a,
        with("Jordan", Some("sam@example.test"), Source::Idp),
    )
    .await;
    create(
        &pool,
        a,
        with("Lisa", Some("lisa@example.test"), Source::Local),
    )
    .await;
    let blair = create(&pool, a, with("Blair", None, Source::Idp)).await;
    users::set_status(
        &pool,
        scope(a),
        blair.id,
        Status::Suspended,
        later_time(),
        Actor::Instance,
    )
    .await
    .unwrap();
    let list = |filter: ListFilter| {
        let pool = pool.clone();
        async move {
            users::list(&pool, scope(a), &filter, None, 10)
                .await
                .unwrap()
        }
    };

    let by_q = list(ListFilter {
        q: Some("SA".into()),
        ..ListFilter::default()
    })
    .await;
    let suspended = list(ListFilter {
        status: Some(Status::Suspended),
        ..ListFilter::default()
    })
    .await;
    let idp_active = list(ListFilter {
        status: Some(Status::Active),
        source: Some(Source::Idp),
        ..ListFilter::default()
    })
    .await;

    assert_eq!(
        names(&by_q),
        ["Jordan", "Sam Ortiz"],
        "prefix of name or email, any case; not Lisa"
    );
    assert_eq!(names(&suspended), ["Blair"]);
    assert_eq!(names(&idp_active), ["Jordan"]);
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_list_prefix_matches_wildcards_literally(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    create(&pool, a, new_user("50% off")).await;
    create(&pool, a, new_user("500 club")).await;
    create(&pool, a, new_user("a_b")).await;
    create(&pool, a, new_user("axb")).await;
    let list = |q: &str| {
        let pool = pool.clone();
        let filter = ListFilter {
            q: Some(q.into()),
            ..ListFilter::default()
        };
        async move {
            users::list(&pool, scope(a), &filter, None, 10)
                .await
                .unwrap()
        }
    };

    assert_eq!(names(&list("50%").await), ["50% off"]);
    assert_eq!(names(&list("a_").await), ["a_b"]);
    assert!(list("%").await.is_empty(), "% matches only a literal %");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_database_refuses_an_unknown_source_or_status(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let insert = |source: &'static str, status: &'static str| {
        sqlx::query(
            "INSERT INTO users (id, org_id, source, display_name, status, tz)
             VALUES (gen_random_uuid(), $1, $2, 'Sam', $3, 'UTC')",
        )
        .bind(a)
        .bind(source)
        .bind(status)
        .execute(&pool)
    };

    assert_eq!(
        sqlstate(insert("ldap", "active").await),
        "23514",
        "T3: check_violation"
    );
    assert_eq!(sqlstate(insert("local", "deleted").await), "23514");
    insert("local", "active")
        .await
        .expect("a known source and status are accepted");
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn the_database_refuses_a_blank_display_name(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;

    let result = sqlx::query(
        "INSERT INTO users (id, org_id, source, display_name, tz)
         VALUES (gen_random_uuid(), $1, 'local', '   ', 'UTC')",
    )
    .bind(a)
    .execute(&pool)
    .await;

    assert_eq!(sqlstate(result), "23514", "check_violation");
}

async fn set_manager(
    pool: &PgPool,
    user: Uuid,
    manager: Uuid,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE users SET manager_id = $2 WHERE id = $1")
        .bind(user)
        .bind(manager)
        .execute(pool)
        .await
}

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn a_manager_is_another_user_of_the_same_organization(pool: PgPool) {
    let a = insert_org(&pool, "org-a").await;
    let b = insert_org(&pool, "org-b").await;
    let user = create(&pool, a, new_user("Sam")).await;
    let manager_a = create(&pool, a, new_user("Manager A")).await;
    let manager_b = create(&pool, b, new_user("Manager B")).await;

    let across = set_manager(&pool, user.id, manager_b.id).await;
    let itself = set_manager(&pool, user.id, user.id).await;

    assert_eq!(
        sqlstate(across),
        "23503",
        "T8: foreign_key_violation across organizations"
    );
    assert_eq!(
        sqlstate(itself),
        "23514",
        "T8: check_violation for a self-reference"
    );
    set_manager(&pool, user.id, manager_a.id)
        .await
        .expect("a manager in the same organization is accepted");
}
