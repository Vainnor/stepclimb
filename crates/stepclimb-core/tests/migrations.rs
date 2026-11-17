// SPDX-License-Identifier: AGPL-3.0-or-later
//! The embedded migrations against a fresh database per test (`#[sqlx::test]`, which needs
//! `DATABASE_URL` pointing at a PostgreSQL server: `just up` starts the Compose one).

use sqlx::PgPool;
use stepclimb_core::db::MIGRATOR;

#[sqlx::test(migrations = false)]
async fn migrations_create_the_organizations_table(pool: PgPool) {
    MIGRATOR
        .run(&pool)
        .await
        .expect("migrations apply to an empty database");

    let columns: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT column_name::text, data_type::text, is_nullable::text
         FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = 'organizations'
         ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .expect("information_schema is readable");

    let expected = [
        ("id", "uuid", "NO"),
        ("slug", "text", "NO"),
        ("name", "text", "NO"),
        ("settings", "jsonb", "NO"),
        ("branding", "jsonb", "NO"),
        ("created_at", "timestamp with time zone", "NO"),
        ("updated_at", "timestamp with time zone", "NO"),
    ];
    let got: Vec<(&str, &str, &str)> = columns
        .iter()
        .map(|(n, t, null)| (n.as_str(), t.as_str(), null.as_str()))
        .collect();
    assert_eq!(got, expected);
}

#[sqlx::test(migrations = false)]
async fn organization_slugs_are_unique(pool: PgPool) {
    MIGRATOR.run(&pool).await.expect("migrations apply");
    let insert = "INSERT INTO organizations (id, slug, name) VALUES (gen_random_uuid(), $1, $2)";
    sqlx::query(insert)
        .bind("vzdc")
        .bind("vZDC")
        .execute(&pool)
        .await
        .expect("first insert");

    let second = sqlx::query(insert)
        .bind("vzdc")
        .bind("Another")
        .execute(&pool)
        .await;

    let error = second.expect_err("a second organization with the same slug is refused");
    let code = error
        .as_database_error()
        .and_then(|e| e.code())
        .map(|c| c.into_owned());
    assert_eq!(
        code.as_deref(),
        Some("23505"),
        "unique_violation, got {error}"
    );
}

#[sqlx::test(migrations = false)]
async fn migrations_are_idempotent_on_rerun(pool: PgPool) {
    MIGRATOR.run(&pool).await.expect("first run");
    MIGRATOR
        .run(&pool)
        .await
        .expect("a second run applies nothing and succeeds");
}

/// Spec 02 B5: every table has `created_at` and `updated_at`, `timestamptz NOT NULL DEFAULT
/// now()`. Walks the schema, so a new migration's table is checked without a test of its own.
#[sqlx::test(migrations = false)]
async fn every_table_has_created_at_and_updated_at(pool: PgPool) {
    MIGRATOR.run(&pool).await.expect("migrations apply");

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
           AND table_name <> '_sqlx_migrations'
         ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .expect("information_schema is readable");
    assert!(
        !tables.is_empty(),
        "the migrations create at least one table"
    );

    let compliant: Vec<(String, i64)> = sqlx::query_as(
        "SELECT table_name::text, count(*)
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND column_name IN ('created_at', 'updated_at')
           AND data_type = 'timestamp with time zone'
           AND is_nullable = 'NO'
           AND column_default = 'now()'
         GROUP BY table_name
         HAVING count(*) = 2",
    )
    .fetch_all(&pool)
    .await
    .expect("information_schema is readable");
    let compliant: Vec<&str> = compliant.iter().map(|(t, _)| t.as_str()).collect();

    let missing: Vec<&str> = tables
        .iter()
        .map(String::as_str)
        .filter(|t| !compliant.contains(t))
        .collect();
    assert!(
        missing.is_empty(),
        "tables without created_at and updated_at as timestamptz NOT NULL DEFAULT now(): {missing:?}"
    );
}

#[sqlx::test(migrations = false)]
async fn a_stored_system_theme_becomes_no_choice_and_other_choices_stay(pool: PgPool) {
    MIGRATOR
        .run_to(20261115220000, &pool)
        .await
        .expect("migrations up to the one before");
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES (gen_random_uuid(), 'a', 'A')")
        .execute(&pool)
        .await
        .expect("an organization");
    for (name, theme) in [("Sam", "system"), ("Lee", "dark")] {
        sqlx::query(
            "INSERT INTO users (id, org_id, display_name, tz, source, theme)
             SELECT gen_random_uuid(), id, $1, 'UTC', 'local', $2 FROM organizations",
        )
        .bind(name)
        .bind(theme)
        .execute(&pool)
        .await
        .expect("a user");
    }
    MIGRATOR.run(&pool).await.expect("the rest apply");
    let themes: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT display_name, theme FROM users ORDER BY display_name")
            .fetch_all(&pool)
            .await
            .expect("the users");
    assert_eq!(
        themes,
        [
            ("Lee".to_owned(), Some("dark".to_owned())),
            ("Sam".to_owned(), None)
        ]
    );
    sqlx::query(
        "INSERT INTO users (id, org_id, display_name, tz, source)
         SELECT gen_random_uuid(), id, 'New', 'UTC', 'local' FROM organizations",
    )
    .execute(&pool)
    .await
    .expect("a user without a theme");
    let new: Option<String> =
        sqlx::query_scalar("SELECT theme FROM users WHERE display_name = 'New'")
            .fetch_one(&pool)
            .await
            .expect("the new user");
    assert_eq!(new, None, "a new user has chosen nothing");
}

/// The `verified_email` migration (spec 07 I4) records the email of every existing local user,
/// whose email an admin wrote, and of no user of another source. The migrations before it run
/// first, rows are inserted, and then it runs.
#[sqlx::test(migrations = false)]
async fn the_verified_email_migration_records_only_local_users(pool: PgPool) {
    const VERSION: i64 = 20261116040000;
    let run = |sql| sqlx::raw_sql(sql).execute(&pool);
    for migration in MIGRATOR.iter().filter(|m| m.version < VERSION) {
        run(migration.sql.clone())
            .await
            .expect("an earlier migration");
    }
    let org = stepclimb_core::id::new_id();
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'org-a', 'Org A')")
        .bind(org)
        .execute(&pool)
        .await
        .expect("insert organization");
    for (source, email) in [
        ("local", Some("Local@Example.test")),
        ("local", None),
        ("idp", Some("idp@example.test")),
        ("sync", Some("sync@example.test")),
    ] {
        sqlx::query(
            "INSERT INTO users (id, org_id, source, display_name, email, tz)
             VALUES ($1, $2, $3, 'Pat', $4, 'UTC')",
        )
        .bind(stepclimb_core::id::new_id())
        .bind(org)
        .bind(source)
        .bind(email)
        .execute(&pool)
        .await
        .expect("insert user");
    }
    let target = MIGRATOR
        .iter()
        .find(|m| m.version == VERSION)
        .expect("the verified_email migration");
    run(target.sql.clone()).await.expect("the migration");

    let records: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT source, email, verified_email FROM users ORDER BY source, email NULLS LAST",
    )
    .fetch_all(&pool)
    .await
    .expect("read users");
    let record = |email: &str| Some(email.to_owned());
    assert_eq!(
        records,
        [
            ("idp".into(), record("idp@example.test"), None),
            (
                "local".into(),
                record("Local@Example.test"),
                record("local@example.test")
            ),
            ("local".into(), None, None),
            ("sync".into(), record("sync@example.test"), None),
        ]
    );
}

/// The `enrollments` migration (M1 spec 02 I2, B22, T18a) removes the Student and Observer
/// assignments at the organization or an account, which M0 allowed, and keeps every other
/// assignment, a course-scope one included. The migrations before it run first, rows are
/// inserted, and then it runs.
#[sqlx::test(migrations = false)]
async fn the_enrollments_migration_removes_course_role_assignments_above_courses(pool: PgPool) {
    const VERSION: i64 = 20261117120000;
    let run = |sql| sqlx::raw_sql(sql).execute(&pool);
    for migration in MIGRATOR.iter().filter(|m| m.version < VERSION) {
        run(migration.sql.clone())
            .await
            .expect("an earlier migration");
    }
    let new_id = stepclimb_core::id::new_id;
    let (org, user, account) = (new_id(), new_id(), new_id());
    sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, 'org-a', 'Org A')")
        .bind(org)
        .execute(&pool)
        .await
        .expect("insert organization");
    sqlx::query(
        "INSERT INTO users (id, org_id, source, display_name, tz)
         VALUES ($1, $2, 'local', 'Pat', 'UTC')",
    )
    .bind(user)
    .bind(org)
    .execute(&pool)
    .await
    .expect("insert user");
    let mut roles = Vec::new();
    for key in ["student", "observer", "instructor"] {
        let id = new_id();
        sqlx::query("INSERT INTO roles (id, key, name, builtin) VALUES ($1, $2, $2, true)")
            .bind(id)
            .bind(key)
            .execute(&pool)
            .await
            .expect("insert built-in role");
        roles.push(id);
    }
    let custom = new_id();
    sqlx::query(
        "INSERT INTO roles (id, org_id, key, name, builtin) VALUES ($1, $2, 'student', 'S', false)",
    )
    .bind(custom)
    .bind(org)
    .execute(&pool)
    .await
    .expect("insert a custom role keyed student");
    let (student, observer, instructor) = (roles[0], roles[1], roles[2]);
    let rows = [
        ("student at org", student, "organization", org),
        ("student at account", student, "account", account),
        ("observer at account", observer, "account", account),
        ("student at course", student, "course", new_id()),
        ("instructor at org", instructor, "organization", org),
        ("instructor at account", instructor, "account", account),
        ("custom student at org", custom, "organization", org),
    ];
    let mut ids = Vec::new();
    for (name, role, scope_type, scope_id) in rows {
        let id = new_id();
        sqlx::query(
            "INSERT INTO role_assignments (id, org_id, user_id, role_id, scope_type, scope_id)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id)
        .bind(org)
        .bind(user)
        .bind(role)
        .bind(scope_type)
        .bind(scope_id)
        .execute(&pool)
        .await
        .expect(name);
        ids.push((name, id));
    }
    let target = MIGRATOR
        .iter()
        .find(|m| m.version == VERSION)
        .expect("the enrollments migration");
    run(target.sql.clone()).await.expect("the migration");

    let kept: Vec<stepclimb_core::id::Uuid> =
        sqlx::query_scalar("SELECT id FROM role_assignments ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("read assignments");
    let kept: Vec<&str> = ids
        .iter()
        .filter(|(_, id)| kept.contains(id))
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        kept,
        [
            "student at course",
            "instructor at org",
            "instructor at account",
            "custom student at org",
        ]
    );
}
