// SPDX-License-Identifier: AGPL-3.0-or-later
//! Keys from `id::new_id` keep their creation order inside PostgreSQL, which keyset pagination
//! by `id` relies on (spec 02 B4, T4). Needs `DATABASE_URL` (`just up`).

use sqlx::PgPool;
use stepclimb_core::id::{Uuid, new_id};

#[sqlx::test(migrator = "stepclimb_core::db::MIGRATOR")]
async fn keys_inserted_in_sequence_come_back_in_creation_order(pool: PgPool) {
    let created: Vec<Uuid> = (0..200).map(|_| new_id()).collect();
    // Insert in reverse so the heap order cannot stand in for the key order.
    for (n, id) in created.iter().enumerate().rev() {
        sqlx::query("INSERT INTO organizations (id, slug, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(format!("org-{n}"))
            .bind(format!("Organization {n}"))
            .execute(&pool)
            .await
            .expect("insert");
    }

    let sorted: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM organizations ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("select");

    assert_eq!(sorted, created);
    assert!(sorted.iter().all(|id| id.get_version_num() == 7));
}
