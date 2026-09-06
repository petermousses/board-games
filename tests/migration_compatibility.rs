use std::env;

use board_games::{
    domain::{GameAction, GameType, checkers::CheckersMove, solitaire::SolitaireAction},
    store::Store,
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
    // Covers both first upgrade and simultaneous replica migration startup.
    let (first, second) = tokio::join!(store.migrate(), store.migrate());
    first.expect("first migration");
    second.expect("concurrent migration");
    store.migrate().await.expect("idempotent migration");
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
        let updated = store
            .apply_action(session_id, &token, &action, None)
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
