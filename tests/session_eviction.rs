use std::env;

use board_games::{domain::GameType, store::Store};
use sqlx::PgPool;
use uuid::Uuid;

#[tokio::test]
async fn evicts_only_sessions_older_than_seven_days_and_dependents() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let admin = PgPool::connect(&database_url).await.expect("postgres");
    let schema = format!("session_eviction_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("isolated schema");
    let separator = if database_url.contains('?') { '&' } else { '?' };
    let isolated_url = format!("{database_url}{separator}options=-csearch_path%3D{schema}");
    let pool = PgPool::connect(&isolated_url)
        .await
        .expect("isolated postgres");
    let store = Store::connect(&isolated_url).await.expect("store");
    store.migrate().await.expect("migrate");

    let stale = store
        .create_session(GameType::Checkers, "stale host".to_owned())
        .await
        .expect("stale session");
    let fresh = store
        .create_session(GameType::Checkers, "fresh host".to_owned())
        .await
        .expect("fresh session");
    let stale_id = stale.session.id;
    let fresh_id = fresh.session.id;
    let stale_participant_id = stale.session.you.id;
    sqlx::query(
        "UPDATE game_sessions SET created_at = NOW() - INTERVAL '7 days' - INTERVAL '1 second' \
         WHERE id = $1",
    )
    .bind(stale_id)
    .execute(&pool)
    .await
    .expect("age stale session");
    sqlx::query(
        "UPDATE game_sessions SET created_at = NOW() - INTERVAL '7 days' + INTERVAL '1 second' \
         WHERE id = $1",
    )
    .bind(fresh_id)
    .execute(&pool)
    .await
    .expect("age fresh session");
    sqlx::query(
        "INSERT INTO game_events (session_id, state_version, participant_id, action) \
         VALUES ($1, 1, $2, '{}'::jsonb)",
    )
    .bind(stale_id)
    .bind(stale_participant_id)
    .execute(&pool)
    .await
    .expect("stale event");

    assert_eq!(store.evict_stale_sessions().await.expect("eviction"), 1);
    assert_counts(&pool, stale_id, (0, 0, 0)).await;
    assert_counts(&pool, fresh_id, (1, 1, 0)).await;

    drop(store);
    drop(pool);
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("drop isolated schema");
}

#[tokio::test]
async fn locked_session_is_skipped_until_in_flight_transaction_finishes() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let admin = PgPool::connect(&database_url).await.expect("postgres");
    let schema = format!("session_eviction_lock_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("isolated schema");
    let separator = if database_url.contains('?') { '&' } else { '?' };
    let isolated_url = format!("{database_url}{separator}options=-csearch_path%3D{schema}");
    let pool = PgPool::connect(&isolated_url)
        .await
        .expect("isolated postgres");
    let store = Store::connect(&isolated_url).await.expect("store");
    store.migrate().await.expect("migrate");
    let stale = store
        .create_session(GameType::Checkers, "locked host".to_owned())
        .await
        .expect("stale session");
    let stale_id = stale.session.id;
    sqlx::query("UPDATE game_sessions SET created_at = NOW() - INTERVAL '8 days' WHERE id = $1")
        .bind(stale_id)
        .execute(&pool)
        .await
        .expect("age stale session");

    let mut in_flight = pool.begin().await.expect("in-flight transaction");
    sqlx::query("SELECT id FROM game_sessions WHERE id = $1 FOR UPDATE")
        .bind(stale_id)
        .fetch_one(&mut *in_flight)
        .await
        .expect("lock session");
    assert_eq!(
        store.evict_stale_sessions().await.expect("first eviction"),
        0
    );
    assert_counts(&pool, stale_id, (1, 1, 0)).await;
    in_flight
        .commit()
        .await
        .expect("finish in-flight transaction");

    assert_eq!(
        store.evict_stale_sessions().await.expect("second eviction"),
        1
    );
    assert_counts(&pool, stale_id, (0, 0, 0)).await;

    drop(store);
    drop(pool);
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("drop isolated schema");
}

async fn assert_counts(pool: &PgPool, session_id: Uuid, expected: (i64, i64, i64)) {
    let actual = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
           (SELECT COUNT(*) FROM game_sessions WHERE id = $1), \
           (SELECT COUNT(*) FROM session_participants WHERE session_id = $1), \
           (SELECT COUNT(*) FROM game_events WHERE session_id = $1)",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await
    .expect("session counts");
    assert_eq!(actual, expected);
}
