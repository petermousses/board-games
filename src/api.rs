use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tracing::error;
use uuid::Uuid;

use crate::{
    domain::{GameAction, GameType},
    store::{SessionAccess, SessionView, Store, StoreError},
};

#[derive(Clone)]
struct AppState {
    store: Store,
}

pub fn router(store: Store) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(readiness))
        .route("/api/v1/sessions", post(create_session))
        .route("/api/v1/sessions/{id}", get(get_session))
        .route("/api/v1/sessions/{id}/join", post(join_session))
        .route("/api/v1/sessions/{id}/actions", post(apply_action))
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024))
        .with_state(AppState { store })
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn readiness(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state
        .store
        .health_check()
        .await
        .map_err(ApiError::unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionAccess>), ApiError> {
    validate_display_name(&request.display_name)?;
    let session = state
        .store
        .create_session(request.game_type, request.display_name)
        .await
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<SessionView>, ApiError> {
    let access_token = bearer_token(&headers)?;
    let session = state
        .store
        .load_authorized(id, &access_token)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(session))
}

async fn join_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(request): Json<JoinSessionRequest>,
) -> Result<(StatusCode, Json<SessionAccess>), ApiError> {
    validate_display_name(&request.display_name)?;
    let session = state
        .store
        .join_checkers(id, request.display_name)
        .await
        .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(session)))
}

async fn apply_action(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<ActionRequest>,
) -> Result<Json<SessionView>, ApiError> {
    let access_token = bearer_token(&headers)?;
    let session = state
        .store
        .apply_action(id, &access_token, &request.action)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(session))
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    game_type: GameType,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct JoinSessionRequest {
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct ActionRequest {
    action: GameAction,
}

fn validate_display_name(display_name: &str) -> Result<(), ApiError> {
    let valid = (1..=32).contains(&display_name.chars().count())
        && display_name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.')
        });
    if valid {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "display_name must be 1-32 ASCII letters, digits, spaces, hyphens, underscores, or periods",
        ))
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<String, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| {
            value.len() == 43
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .ok_or_else(|| ApiError::unauthorized("a valid bearer access token is required"))?;
    Ok(token.to_owned())
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: &'static str,
}

impl ApiError {
    fn bad_request(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn unauthorized(message: &'static str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message,
        }
    }

    fn unavailable(error: StoreError) -> Self {
        error!(error = %error, "readiness check failed");
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "database is not ready",
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::NotFound => Self {
                status: StatusCode::NOT_FOUND,
                message: "session does not exist",
            },
            StoreError::Unauthorized => Self::unauthorized("session access is unauthorized"),
            StoreError::Conflict(message) => Self {
                status: StatusCode::CONFLICT,
                message,
            },
            StoreError::InvalidAction(_) => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: "the game action is not legal",
            },
            StoreError::Database(error) => {
                error!(error = %error, "database operation failed");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "internal server error",
                }
            }
            StoreError::Migration(error) => {
                error!(error = %error, "database migration failed");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "internal server error",
                }
            }
            StoreError::Serialization(error) => {
                error!(error = %error, "session serialization failed");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "internal server error",
                }
            }
            StoreError::CorruptData(message) => {
                error!(reason = message, "stored session data is invalid");
                Self {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: "internal server error",
                }
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
}
