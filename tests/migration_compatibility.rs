use std::env;

use board_games::{
    domain::{GameAction, GameType, checkers::CheckersMove, solitaire::SolitaireAction},
    store::{MIGRATOR, Store, StoreError},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

#[tokio::test]
async fn additive_migration_preserves_legacy_games_tokens_and_events() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL");
    let admin = PgPool::connect(&database_url).await.expect("postgres");
    let schema = format!("migration_test_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("isolated schema");
    let separator = if database_url.contains('?') { '&' } else { '?' };
    let legacy_url = format!("{database_url}{separator}options=-csearch_path%3D{schema}");
    let legacy_pool = PgPool::connect(&legacy_url)
        .await
        .expect("isolated connection");
    let migration_dir = env::temp_dir().join(&schema);
    std::fs::create_dir(&migration_dir).expect("migration directory");
    std::fs::write(
        migration_dir.join("0001_initial.sql"),
        include_str!("../migrations/0001_initial.sql"),
    )
    .expect("original migration");
    let old_migrator = sqlx::migrate::Migrator::new(migration_dir.as_path())
        .await
        .expect("old migrator");
    old_migrator
        .run(&legacy_pool)
        .await
        .expect("migrate original schema only");
    std::fs::remove_dir_all(&migration_dir).expect("remove migration fixture directory");

    let token = "A".repeat(43);
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();
    let mut fixtures = Vec::new();
    for game_type in [GameType::Solitaire, GameType::Checkers] {
        let session_id = Uuid::new_v4();
        let participant_id = Uuid::new_v4();
        // Construct the original JSON contract independently of the new serializer.
        let raw = legacy_snapshot(game_type);
        sqlx::query("INSERT INTO game_sessions (id,game_type,state,state_version,status) VALUES ($1,$2::game_type,$3,1,'active')")
            .bind(session_id).bind(game_type.as_db()).bind(Json(raw.clone())).execute(&legacy_pool).await.unwrap();
        let seat = if game_type == GameType::Solitaire {
            "solitaire"
        } else {
            "red"
        };
        sqlx::query("INSERT INTO session_participants (id,session_id,seat,display_name,token_hash) VALUES ($1,$2,$3,'legacy player',$4)")
            .bind(participant_id).bind(session_id).bind(seat).bind(&token_hash).execute(&legacy_pool).await.unwrap();
        if game_type == GameType::Checkers {
            sqlx::query("INSERT INTO session_participants (id,session_id,seat,display_name,token_hash) VALUES ($1,$2,'black','legacy opponent',$3)")
                .bind(Uuid::new_v4()).bind(session_id).bind(Sha256::digest(b"B".repeat(43)).to_vec()).execute(&legacy_pool).await.unwrap();
        }
        sqlx::query("INSERT INTO game_events (session_id,state_version,participant_id,action) VALUES ($1,1,$2,$3)")
            .bind(session_id).bind(participant_id).bind(Json(json!({"legacy":"preserved event"}))).execute(&legacy_pool).await.unwrap();
        fixtures.push((session_id, game_type, raw));
    }

    let store = Store::connect(&legacy_url).await.unwrap();
    assert!(
        store.health_check().await.is_err(),
        "readiness must reject a schema missing an embedded migration"
    );
    // Covers both first upgrade and simultaneous replica migration startup.
    let (first, second) = tokio::join!(store.migrate(), store.migrate());
    first.expect("first migration");
    second.expect("concurrent migration");
    store.migrate().await.expect("idempotent migration");
    store
        .health_check()
        .await
        .expect("readiness after normal migration");
    for (session_id, game_type, raw) in fixtures {
        let loaded = store
            .load_authorized(session_id, &token)
            .await
            .expect("resume legacy token");
        assert_eq!(loaded.state, raw);
        assert_eq!(loaded.state_version, 1);
        assert_eq!(loaded.you.player_index, 0);
        let action = match game_type {
            GameType::Solitaire => GameAction::Solitaire(SolitaireAction::Draw),
            GameType::Checkers => GameAction::Checkers(CheckersMove {
                from: 20,
                path: vec![16],
            }),
            _ => unreachable!(),
        };
        let negative_version = store
            .apply_action(session_id, &token, &action, -1)
            .await
            .expect_err("negative versions must be rejected before mutation");
        assert!(matches!(
            negative_version,
            StoreError::BadRequest("expected_version must be nonnegative")
        ));
        let after_rejected_version = store
            .load_authorized(session_id, &token)
            .await
            .expect("legacy game is unchanged after rejected version");
        assert_eq!(after_rejected_version.state, raw);
        assert_eq!(after_rejected_version.state_version, 1);
        let current_version = after_rejected_version.state_version;
        let rejected_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM game_events WHERE session_id=$1")
                .bind(session_id)
                .fetch_one(&legacy_pool)
                .await
                .unwrap();
        assert_eq!(rejected_count, 1);
        let updated = store
            .apply_action(session_id, &token, &action, current_version)
            .await
            .expect("continue legacy game");
        assert_eq!(updated.state_version, 2);
        let old: Json<serde_json::Value> = sqlx::query_scalar(
            "SELECT action FROM game_events WHERE session_id=$1 AND state_version=1",
        )
        .bind(session_id)
        .fetch_one(&legacy_pool)
        .await
        .unwrap();
        assert_eq!(old.0, json!({"legacy":"preserved event"}));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM game_events WHERE session_id=$1")
            .bind(session_id)
            .fetch_one(&legacy_pool)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }
    for game_type in [
        GameType::Chess,
        GameType::Battleship,
        GameType::Clue,
        GameType::ConnectFour,
        GameType::Reversi,
        GameType::TicTacToe,
    ] {
        store
            .create_session(game_type, "new game".to_owned())
            .await
            .expect("new enum value usable after upgrade");
    }
    drop(store);
    legacy_pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("remove isolated schema");
}

#[tokio::test]
async fn readiness_rejects_invalid_migration_ledger_entries() {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL");
    let admin = PgPool::connect(&database_url).await.expect("postgres");
    let schema = format!("migration_ready_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .expect("isolated schema");
    let separator = if database_url.contains('?') { '&' } else { '?' };
    let isolated_url = format!("{database_url}{separator}options=-csearch_path%3D{schema}");
    let isolated_pool = PgPool::connect(&isolated_url)
        .await
        .expect("isolated connection");
    let store = Store::connect(&isolated_url)
        .await
        .expect("isolated connection");
    store.migrate().await.expect("normal migration");
    store
        .health_check()
        .await
        .expect("healthy migration ledger");

    let migration = MIGRATOR
        .iter()
        .find(|migration| migration.migration_type.is_up_migration())
        .expect("embedded migration");

    sqlx::query("UPDATE _sqlx_migrations SET success = false WHERE version = $1")
        .bind(migration.version)
        .execute(&isolated_pool)
        .await
        .expect("mark migration unsuccessful");
    assert!(
        store.health_check().await.is_err(),
        "readiness must reject an unsuccessful migration"
    );
    sqlx::query("UPDATE _sqlx_migrations SET success = true WHERE version = $1")
        .bind(migration.version)
        .execute(&isolated_pool)
        .await
        .expect("restore successful migration");

    sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = $2")
        .bind(vec![0_u8])
        .bind(migration.version)
        .execute(&isolated_pool)
        .await
        .expect("change migration checksum");
    assert!(
        store.health_check().await.is_err(),
        "readiness must reject a checksum mismatch"
    );
    sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = $2")
        .bind(migration.checksum.as_ref())
        .bind(migration.version)
        .execute(&isolated_pool)
        .await
        .expect("restore migration checksum");
    store
        .health_check()
        .await
        .expect("restored migration ledger");

    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (9999, 'unexpected', true, $1, 0)",
    )
    .bind(vec![0_u8])
    .execute(&isolated_pool)
    .await
    .expect("insert unexpected migration");
    assert!(
        store.health_check().await.is_err(),
        "readiness must reject an unexpected applied migration"
    );

    drop(store);
    isolated_pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .expect("remove isolated schema");
}

fn legacy_snapshot(game_type: GameType) -> serde_json::Value {
    match game_type {
        GameType::Solitaire => {
            let mut next = 24;
            let tableau = (0..7)
                .map(|index| {
                    let cards = (next..next + index + 1).collect::<Vec<_>>();
                    next += index + 1;
                    json!({"cards":cards,"face_up_from":index})
                })
                .collect::<Vec<_>>();
            json!({"game_type":"solitaire","state":{
                "stock":(0..24).collect::<Vec<_>>(),"waste":[],
                "foundations":[[],[],[],[]],"tableau":tableau,"moves":0,"won":false
            }})
        }
        GameType::Checkers => {
            let board = (0..32)
                .map(|index| {
                    if index < 12 {
                        3
                    } else if index < 20 {
                        0
                    } else {
                        1
                    }
                })
                .collect::<Vec<_>>();
            json!({"game_type":"checkers","state":{
                "board":board,"side_to_move":"red","winner":null,"turn":0
            }})
        }
        _ => unreachable!(),
    }
}
