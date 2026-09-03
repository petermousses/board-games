use std::env;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use board_games::{api, store::Store};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn checkers_join_and_concurrent_moves_are_serialized() {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL is required for the postgres-backed API integration test");
    let store = Store::connect(&database_url)
        .await
        .expect("connect postgres");
    store.migrate().await.expect("migrate postgres");
    let app = api::router(store);

    let created = send_json(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/sessions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({ "game_type": "checkers", "display_name": "red" }).to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED);
    let session_id = created.1["id"].as_str().expect("session id").to_owned();
    let red_token = created.1["access_token"]
        .as_str()
        .expect("red token")
        .to_owned();

    let rejected_lobby_move =
        send_json(&app, action_request(&session_id, &red_token, 20, vec![16])).await;
    assert_eq!(rejected_lobby_move.0, StatusCode::CONFLICT);

    let joined = send_json(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/sessions/{session_id}/join"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "display_name": "black" }).to_string()))
            .expect("request"),
    )
    .await;
    assert_eq!(joined.0, StatusCode::CREATED);
    assert_eq!(joined.1["status"], "active");

    let (first, second) = tokio::join!(
        send_json(&app, action_request(&session_id, &red_token, 20, vec![16]),),
        send_json(&app, action_request(&session_id, &red_token, 20, vec![16]),),
    );
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::UNPROCESSABLE_ENTITY]);

    let resumed = send_json(
        &app,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/sessions/{session_id}"))
            .header(header::AUTHORIZATION, format!("Bearer {red_token}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(resumed.0, StatusCode::OK);
    assert_eq!(resumed.1["state_version"], 1);
    assert_eq!(resumed.1["state"]["state"]["side_to_move"], "black");

    let unauthorized = send_json(
        &app,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/sessions/{session_id}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(unauthorized.0, StatusCode::UNAUTHORIZED);
}

fn action_request(session_id: &str, access_token: &str, from: u8, path: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{session_id}/actions"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .body(Body::from(
            json!({
                "action": {
                    "game_type": "checkers",
                    "action": { "from": from, "path": path }
                }
            })
            .to_string(),
        ))
        .expect("request")
}

async fn send_json(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    (
        status,
        serde_json::from_slice(&body).expect("json response"),
    )
}
