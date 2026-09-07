use std::env;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use board_games::{api, store::Store};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn app() -> Router {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let store = Store::connect(&database_url)
        .await
        .expect("connect postgres");
    store.migrate().await.expect("migrate postgres");
    api::router(store)
}

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let body = if let Some(body) = body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn create(app: &Router, game_type: &str) -> Value {
    let response = request(
        app,
        "POST",
        "/api/v1/sessions",
        Some(json!({"game_type":game_type,"display_name":"host"})),
        None,
    )
    .await;
    assert_eq!(
        response.0,
        StatusCode::CREATED,
        "{game_type}: {}",
        response.1
    );
    response.1
}

async fn join(app: &Router, id: &str) -> (StatusCode, Value) {
    request(
        app,
        "POST",
        &format!("/api/v1/sessions/{id}/join"),
        Some(json!({"display_name":"guest"})),
        None,
    )
    .await
}

fn id(access: &Value) -> &str {
    access["id"].as_str().expect("id")
}
fn token(access: &Value) -> &str {
    access["access_token"].as_str().expect("token")
}

#[tokio::test]
async fn complete_catalog_creates_games_and_assigns_compatible_seats() {
    let app = app().await;
    for game_type in [
        "solitaire",
        "checkers",
        "chess",
        "battleship",
        "clue",
        "connect_four",
        "reversi",
        "tic_tac_toe",
    ] {
        let host = create(&app, game_type).await;
        assert_eq!(host["game_type"], game_type);
        assert_eq!(host["state"]["game_type"], game_type);
        assert_eq!(host["you"]["player_index"], 0);
        assert_eq!(
            host["you"]["seat"],
            match game_type {
                "solitaire" => "solitaire",
                "checkers" => "red",
                _ => "player1",
            }
        );
        let guest = join(&app, id(&host)).await;
        if game_type == "solitaire" {
            assert_eq!(host["status"], "active");
            assert_eq!(guest.0, StatusCode::CONFLICT);
            continue;
        }
        assert_eq!(host["status"], "lobby");
        assert_eq!(guest.0, StatusCode::CREATED, "{game_type}: {}", guest.1);
        assert_eq!(guest.1["you"]["player_index"], 1);
        assert_eq!(
            guest.1["you"]["seat"],
            if game_type == "checkers" {
                "black"
            } else {
                "player2"
            }
        );
        assert_eq!(
            guest.1["status"],
            if game_type == "clue" {
                "lobby"
            } else {
                "active"
            }
        );
        assert_eq!(guest.1["state_version"], 1);
    }
}

async fn get(app: &Router, access: &Value) -> (StatusCode, Value) {
    request(
        app,
        "GET",
        &format!("/api/v1/sessions/{}", id(access)),
        None,
        Some(token(access)),
    )
    .await
}

async fn start(app: &Router, access: &Value) -> (StatusCode, Value) {
    request(
        app,
        "POST",
        &format!("/api/v1/sessions/{}/start", id(access)),
        None,
        Some(token(access)),
    )
    .await
}

async fn action(
    app: &Router,
    access: &Value,
    action: Value,
    expected_version: i64,
) -> (StatusCode, Value) {
    let body = json!({
        "expected_version": expected_version,
        "action": {"game_type":access["game_type"],"action":action},
    });
    request(
        app,
        "POST",
        &format!("/api/v1/sessions/{}/actions", id(access)),
        Some(body),
        Some(token(access)),
    )
    .await
}

async fn action_at_current_version(
    app: &Router,
    access: &Value,
    game_action: Value,
) -> (StatusCode, Value) {
    let session = get(app, access).await.1;
    let expected_version = session["state_version"].as_i64().expect("state version");
    action(app, access, game_action, expected_version).await
}

async fn pool() -> sqlx::PgPool {
    sqlx::PgPool::connect(&env::var("DATABASE_URL").expect("DATABASE_URL"))
        .await
        .expect("postgres")
}

async fn event_count(session: &Value) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM game_events WHERE session_id = $1")
        .bind(uuid::Uuid::parse_str(id(session)).expect("uuid"))
        .fetch_one(&pool().await)
        .await
        .expect("events")
}

#[tokio::test]
async fn bearer_tokens_cannot_read_act_on_or_start_other_sessions() {
    let app = app().await;
    let host = create(&app, "clue").await;
    let other = create(&app, "clue").await;
    for token in [None, Some("malformed"), Some(token(&other))] {
        for (method, suffix, body) in [
            ("GET", "", None),
            ("POST", "/start", None),
            (
                "POST",
                "/actions",
                Some(json!({
                    "expected_version": 0,
                    "action":{"game_type":"clue","action":{"kind":"end_turn"}},
                })),
            ),
        ] {
            let response = request(
                &app,
                method,
                &format!("/api/v1/sessions/{}{suffix}", id(&host)),
                body,
                token,
            )
            .await;
            assert_eq!(response.0, StatusCode::UNAUTHORIZED);
            assert_eq!(response.1.as_object().expect("error").len(), 1);
        }
    }
    let before = get(&app, &host).await.1;
    assert_eq!(start(&app, &host).await.0, StatusCode::CONFLICT);
    let guest = join(&app, id(&host)).await.1;
    assert_eq!(start(&app, &guest).await.0, StatusCode::FORBIDDEN);
    let after = get(&app, &host).await.1;
    assert_eq!(after["state"], before["state"]);
    assert_eq!(after["status"], "lobby");
    assert_eq!(event_count(&host).await, 0);
}

#[tokio::test]
async fn final_seat_joins_are_serialized_and_clue_capacity_is_six() {
    let app = app().await;
    let host = create(&app, "chess").await;
    let (first, second) = tokio::join!(join(&app, id(&host)), join(&app, id(&host)));
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);
    assert_eq!(
        get(&app, &host).await.1["participants"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let host = create(&app, "clue").await;
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..7 {
        let app = app.clone();
        let session_id = id(&host).to_owned();
        jobs.spawn(async move { join(&app, &session_id).await.0 });
    }
    let mut accepted = 0;
    while let Some(result) = jobs.join_next().await {
        match result.expect("join task") {
            StatusCode::CREATED => accepted += 1,
            StatusCode::CONFLICT => {}
            unexpected => panic!("unexpected join response: {unexpected}"),
        }
    }
    assert_eq!(accepted, 5);
    let fresh = get(&app, &host).await.1;
    assert_eq!(fresh["state_version"], 5);
    assert_eq!(fresh["status"], "lobby");
    for (index, participant) in fresh["participants"].as_array().unwrap().iter().enumerate() {
        assert_eq!(participant["player_index"], index);
    }
    let started = start(&app, &host).await;
    assert_eq!(started.0, StatusCode::OK);
    assert_eq!(started.1["state"]["state"]["players"], 6);
    assert_eq!(started.1["state_version"], 6);
    assert_eq!(start(&app, &host).await.0, StatusCode::CONFLICT);
    assert_eq!(join(&app, id(&host)).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn racing_clue_start_and_join_never_produces_an_undealt_active_player() {
    let app = app().await;
    let host = create(&app, "clue").await;
    assert_eq!(join(&app, id(&host)).await.0, StatusCode::CREATED);
    assert_eq!(join(&app, id(&host)).await.0, StatusCode::CREATED);
    let (started, joined) = tokio::join!(start(&app, &host), join(&app, id(&host)));
    assert_eq!(started.0, StatusCode::OK);
    assert!(matches!(
        joined.0,
        StatusCode::CREATED | StatusCode::CONFLICT
    ));
    let fresh = get(&app, &host).await.1;
    let count = fresh["participants"].as_array().unwrap().len();
    assert_eq!(
        count,
        if joined.0 == StatusCode::CREATED {
            4
        } else {
            3
        }
    );
    assert_eq!(fresh["state"]["state"]["players"], count);
    assert_eq!(fresh["status"], "active");
    assert_eq!(fresh["state_version"], count);
    assert_eq!(event_count(&host).await, 1);
}

#[tokio::test]
async fn action_versions_are_required_and_replay_safe() {
    let app = app().await;
    let host = create(&app, "checkers").await;
    let guest = join(&app, id(&host)).await.1;
    let before = get(&app, &host).await.1;
    let movement = json!({"from":20,"path":[16]});
    let missing = request(
        &app,
        "POST",
        &format!("/api/v1/sessions/{}/actions", id(&host)),
        Some(json!({"action":{"game_type":"checkers","action":movement.clone()}})),
        Some(token(&host)),
    )
    .await;
    assert_eq!(missing.0, StatusCode::BAD_REQUEST);
    assert_eq!(missing.1, json!({"error":"expected_version is required"}));
    assert_eq!(get(&app, &host).await.1, before);
    assert_eq!(event_count(&host).await, 0);

    let negative = action(&app, &host, movement.clone(), -1).await;
    assert_eq!(negative.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        negative.1,
        json!({"error":"expected_version must be nonnegative"})
    );
    assert_eq!(get(&app, &host).await.1, before);
    assert_eq!(event_count(&host).await, 0);

    assert_eq!(
        action(&app, &host, movement.clone(), 0).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        action(&app, &guest, json!({"from":9,"path":[13]}), 1)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(get(&app, &host).await.1, before);
    assert_eq!(event_count(&host).await, 0);

    let (first, second) = tokio::join!(
        action(&app, &host, movement.clone(), 1),
        action(&app, &host, movement.clone(), 1),
    );
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    let fresh = get(&app, &host).await.1;
    assert_eq!(fresh["state_version"], 2);
    assert_eq!(fresh["state"]["state"]["side_to_move"], "black");
    assert_eq!(event_count(&host).await, 1);
    let replay = action(&app, &host, movement, 1).await;
    assert_eq!(replay.0, StatusCode::CONFLICT);
    assert_eq!(
        replay.1,
        json!({"error":"the board changed; refresh before trying again"})
    );
    assert_eq!(get(&app, &host).await.1, fresh);
    assert_eq!(event_count(&host).await, 1);
    assert_eq!(
        action(&app, &guest, json!({"from":9,"path":[13]}), 2)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(get(&app, &host).await.1["state_version"], 3);
    assert_eq!(event_count(&host).await, 2);
}

#[tokio::test]
async fn event_insert_failure_rolls_back_the_game_snapshot() {
    let app = app().await;
    let host = create(&app, "checkers").await;
    assert_eq!(join(&app, id(&host)).await.0, StatusCode::CREATED);
    let before = get(&app, &host).await.1;
    let database = pool().await;
    // Reserve the next event version to force a real database failure AFTER the snapshot update.
    sqlx::query("INSERT INTO game_events (session_id,state_version,participant_id,action) VALUES ($1,2,$2,$3)")
        .bind(uuid::Uuid::parse_str(id(&host)).unwrap())
        .bind(uuid::Uuid::parse_str(host["you"]["id"].as_str().unwrap()).unwrap())
        .bind(sqlx::types::Json(json!({"fault":"reserved version"})))
        .execute(&database).await.expect("fault injection");
    let response = action(&app, &host, json!({"from":20,"path":[16]}), 1).await;
    assert_eq!(response.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.1, json!({"error":"internal server error"}));
    assert_eq!(get(&app, &host).await.1, before);
    assert_eq!(event_count(&host).await, 1);
}

fn assert_no_private_keys(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    !["seed", "solution", "hands", "fleets", "token_hash"].contains(&key.as_str()),
                    "private field leaked: {key}"
                );
                assert_no_private_keys(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_no_private_keys),
        _ => {}
    }
}

#[tokio::test]
async fn clue_responses_always_redact_other_hands_and_the_solution() {
    let app = app().await;
    let host = create(&app, "clue").await;
    assert_no_private_keys(&host);
    let guest = join(&app, id(&host)).await.1;
    let third = join(&app, id(&host)).await.1;
    assert_no_private_keys(&guest);
    assert_no_private_keys(&third);
    let started = start(&app, &host).await;
    assert_eq!(started.0, StatusCode::OK);
    assert_no_private_keys(&started.1);
    let stored: sqlx::types::Json<Value> =
        sqlx::query_scalar("SELECT state FROM game_sessions WHERE id=$1")
            .bind(uuid::Uuid::parse_str(id(&host)).unwrap())
            .fetch_one(&pool().await)
            .await
            .unwrap();
    let raw = &stored.0["state"];
    let hands = raw["hands"].as_array().expect("private hands persisted");
    let mut seen_cards = std::collections::HashSet::new();
    for (index, access) in [&host, &guest, &third].into_iter().enumerate() {
        let response = get(&app, access).await;
        assert_eq!(response.0, StatusCode::OK);
        assert_no_private_keys(&response.1);
        let own = &response.1["state"]["state"]["your_hand"];
        assert_eq!(own, &hands[index]);
        for card in own.as_array().expect("own hand") {
            assert!(
                seen_cards.insert(card.to_string()),
                "a card was dealt to multiple players"
            );
        }
    }
    assert_eq!(seen_cards.len(), 18);
    let moved = action_at_current_version(&app, &host, json!({"kind":"end_turn"})).await;
    assert_eq!(moved.0, StatusCode::OK);
    assert_no_private_keys(&moved.1);
}

#[tokio::test]
async fn battleship_fleets_stay_private_during_setup_and_play() {
    let app = app().await;
    let host = create(&app, "battleship").await;
    assert_no_private_keys(&host);
    let guest = join(&app, id(&host)).await.1;
    assert_no_private_keys(&guest);
    let host_fleet = json!({"kind":"place_fleet","ships":(0..5).map(|row| json!({"row":row,"column":0,"horizontal":true})).collect::<Vec<_>>()});
    let placed = action_at_current_version(&app, &host, host_fleet).await;
    assert_eq!(placed.0, StatusCode::OK);
    assert_no_private_keys(&placed.1);
    assert_eq!(
        placed.1["state"]["state"]["own_board"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|cell| **cell == 1)
            .count(),
        17
    );
    let guest_before = get(&app, &guest).await.1;
    assert!(
        guest_before["state"]["state"]["target_board"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cell| *cell == 0)
    );
    assert!(
        guest_before["state"]["state"]["own_board"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cell| *cell == 0)
    );
    assert_no_private_keys(&guest_before);
    assert_eq!(
        action_at_current_version(&app, &host, json!({"kind":"fire","row":5,"column":4}))
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let guest_fleet = json!({"kind":"place_fleet","ships":(5..10).map(|row| json!({"row":row,"column":4,"horizontal":true})).collect::<Vec<_>>()});
    let placed_guest = action_at_current_version(&app, &guest, guest_fleet).await;
    assert_eq!(placed_guest.0, StatusCode::OK);
    assert_no_private_keys(&placed_guest.1);
    let fired =
        action_at_current_version(&app, &host, json!({"kind":"fire","row":5,"column":4})).await;
    assert_eq!(fired.0, StatusCode::OK);
    assert_no_private_keys(&fired.1);
    let targets = fired.1["state"]["state"]["target_board"]
        .as_array()
        .unwrap();
    assert_eq!(targets[54], 3);
    assert_eq!(targets.iter().filter(|cell| **cell != 0).count(), 1);
    let guest_after = get(&app, &guest).await.1;
    assert_no_private_keys(&guest_after);
    assert_eq!(guest_after["state"]["state"]["own_board"][54], 3);
    assert!(
        guest_after["state"]["state"]["target_board"]
            .as_array()
            .unwrap()
            .iter()
            .all(|cell| *cell == 0)
    );
    let persisted: JsonValue = sqlx::query_scalar("SELECT state FROM game_sessions WHERE id=$1")
        .bind(uuid::Uuid::parse_str(id(&host)).unwrap())
        .fetch_one(&pool().await)
        .await
        .unwrap();
    assert_eq!(persisted.0["state"]["fleets"].as_array().unwrap().len(), 2);
}

type JsonValue = sqlx::types::Json<Value>;

#[tokio::test]
async fn terminal_games_reject_actions_and_mismatched_game_actions_never_mutate_state() {
    let app = app().await;
    let host = create(&app, "tic_tac_toe").await;
    let guest = join(&app, id(&host)).await.1;
    let before = get(&app, &host).await.1;
    let expected_version = before["state_version"].as_i64().expect("state version");
    let wrong = request(
        &app,
        "POST",
        &format!("/api/v1/sessions/{}/actions", id(&host)),
        Some(json!({
            "expected_version": expected_version,
            "action":{"game_type":"checkers","action":{"from":20,"path":[16]}},
        })),
        Some(token(&host)),
    )
    .await;
    assert_eq!(wrong.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(get(&app, &host).await.1, before);
    assert_eq!(event_count(&host).await, 0);
    for (access, square) in [(&host, 0), (&guest, 3), (&host, 1), (&guest, 4), (&host, 2)] {
        assert_eq!(
            action_at_current_version(&app, access, json!({"kind":"place","square":square}))
                .await
                .0,
            StatusCode::OK
        );
    }
    let completed = get(&app, &host).await.1;
    assert_eq!(completed["status"], "complete");
    assert_eq!(completed["state"]["state"]["winner"], 0);
    assert_eq!(completed["state_version"], 6);
    assert_eq!(
        action_at_current_version(&app, &guest, json!({"kind":"place","square":8}))
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(join(&app, id(&host)).await.0, StatusCode::CONFLICT);
    assert_eq!(get(&app, &host).await.1, completed);
    assert_eq!(event_count(&host).await, 5);
}
