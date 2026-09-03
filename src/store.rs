use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions, types::Json};
use uuid::Uuid;

use crate::domain::{GameAction, GameState, GameType, checkers::Side};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub id: Uuid,
    pub game_type: GameType,
    pub state: GameState,
    pub state_version: i64,
    pub status: SessionStatus,
    pub you: Participant,
    pub participants: Vec<Participant>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionAccess {
    pub access_token: String,
    #[serde(flatten)]
    pub session: SessionView,
}

#[derive(Debug, Clone, Serialize)]
pub struct Participant {
    pub id: Uuid,
    pub seat: Seat,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    Solitaire,
    Red,
    Black,
}

impl Seat {
    fn as_db(self) -> &'static str {
        match self {
            Self::Solitaire => "solitaire",
            Self::Red => "red",
            Self::Black => "black",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "solitaire" => Ok(Self::Solitaire),
            "red" => Ok(Self::Red),
            "black" => Ok(Self::Black),
            _ => Err(StoreError::CorruptData("unknown participant seat")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Lobby,
    Active,
    Complete,
}

impl SessionStatus {
    fn as_db(self) -> &'static str {
        match self {
            Self::Lobby => "lobby",
            Self::Active => "active",
            Self::Complete => "complete",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "lobby" => Ok(Self::Lobby),
            "active" => Ok(Self::Active),
            "complete" => Ok(Self::Complete),
            _ => Err(StoreError::CorruptData("unknown session status")),
        }
    }
}

impl Store {
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), StoreError> {
        let mut connection = self.pool.acquire().await?;
        // Multiple API replicas may start together. This session-level lock serializes
        // migrations and is released automatically if the database connection dies.
        sqlx::query("SELECT pg_advisory_lock(716201491)")
            .execute(&mut *connection)
            .await?;
        let migration_result = MIGRATOR.run(&mut *connection).await;
        let unlock_result = sqlx::query("SELECT pg_advisory_unlock(716201491)")
            .execute(&mut *connection)
            .await;
        if let Err(error) = migration_result {
            return Err(error.into());
        }
        unlock_result?;
        Ok(())
    }

    pub async fn health_check(&self) -> Result<(), StoreError> {
        // Readiness must fail until the migration job created the authoritative tables.
        sqlx::query("SELECT 1 FROM game_sessions LIMIT 0")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_session(
        &self,
        game_type: GameType,
        display_name: String,
    ) -> Result<SessionAccess, StoreError> {
        let id = Uuid::new_v4();
        let participant_id = Uuid::new_v4();
        let (access_token, token_hash) = new_access_token();
        let seat = match game_type {
            GameType::Solitaire => Seat::Solitaire,
            GameType::Checkers => Seat::Red,
        };
        let status = match game_type {
            GameType::Solitaire => SessionStatus::Active,
            GameType::Checkers => SessionStatus::Lobby,
        };
        let state = GameState::new(game_type, rand::random());
        let state_json = Json(serde_json::to_value(&state)?);

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO game_sessions (id, game_type, state, status) \
             VALUES ($1, $2::game_type, $3, $4::session_status)",
        )
        .bind(id)
        .bind(game_type.as_db())
        .bind(state_json)
        .bind(status.as_db())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO session_participants (id, session_id, seat, display_name, token_hash) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(participant_id)
        .bind(id)
        .bind(seat.as_db())
        .bind(&display_name)
        .bind(token_hash)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        let participant = Participant {
            id: participant_id,
            seat,
            display_name,
        };
        Ok(SessionAccess {
            access_token,
            session: SessionView {
                id,
                game_type,
                state,
                state_version: 0,
                status,
                you: participant.clone(),
                participants: vec![participant],
            },
        })
    }

    pub async fn join_checkers(
        &self,
        id: Uuid,
        display_name: String,
    ) -> Result<SessionAccess, StoreError> {
        let participant_id = Uuid::new_v4();
        let (access_token, token_hash) = new_access_token();
        let mut transaction = self.pool.begin().await?;
        let session = sqlx::query_as::<_, SessionMetadataRow>(
            "SELECT game_type::text AS game_type, status::text AS status \
             FROM game_sessions WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound)?;

        if parse_game_type(&session.game_type)? != GameType::Checkers {
            return Err(StoreError::Conflict(
                "only checkers sessions accept a second player",
            ));
        }
        if SessionStatus::parse(&session.status)? != SessionStatus::Lobby {
            return Err(StoreError::Conflict(
                "this session is no longer waiting for an opponent",
            ));
        }

        sqlx::query(
            "INSERT INTO session_participants (id, session_id, seat, display_name, token_hash) \
             VALUES ($1, $2, 'black', $3, $4)",
        )
        .bind(participant_id)
        .bind(id)
        .bind(&display_name)
        .bind(token_hash)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE game_sessions SET status = 'active', updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        let session = self.load_authorized(id, &access_token).await?;
        Ok(SessionAccess {
            access_token,
            session,
        })
    }

    pub async fn load_authorized(
        &self,
        id: Uuid,
        access_token: &str,
    ) -> Result<SessionView, StoreError> {
        let row = sqlx::query_as::<_, AuthorizedSessionRow>(
            "SELECT s.id, s.game_type::text AS game_type, s.state, s.state_version, \
                    s.status::text AS status, p.id AS participant_id, p.seat, p.display_name \
             FROM game_sessions s \
             INNER JOIN session_participants p ON p.session_id = s.id \
             WHERE s.id = $1 AND p.token_hash = $2",
        )
        .bind(id)
        .bind(hash_access_token(access_token))
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::Unauthorized)?;

        self.view_from_authorized_row(row).await
    }

    pub async fn apply_action(
        &self,
        id: Uuid,
        access_token: &str,
        action: &GameAction,
    ) -> Result<SessionView, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, AuthorizedSessionRow>(
            "SELECT s.id, s.game_type::text AS game_type, s.state, s.state_version, \
                    s.status::text AS status, p.id AS participant_id, p.seat, p.display_name \
             FROM game_sessions s \
             INNER JOIN session_participants p ON p.session_id = s.id \
             WHERE s.id = $1 AND p.token_hash = $2 FOR UPDATE OF s",
        )
        .bind(id)
        .bind(hash_access_token(access_token))
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::Unauthorized)?;

        let game_type = parse_game_type(&row.game_type)?;
        let status = SessionStatus::parse(&row.status)?;
        let seat = Seat::parse(&row.seat)?;
        let mut state: GameState = serde_json::from_value(row.state.0)?;
        if state.game_type() != game_type {
            return Err(StoreError::CorruptData(
                "stored game type and state do not agree",
            ));
        }
        if action.game_type() != game_type {
            return Err(StoreError::InvalidAction(
                "action game type does not match the session".to_owned(),
            ));
        }

        let next_status = match (&mut state, action) {
            (GameState::Solitaire(game), GameAction::Solitaire(action)) => {
                if seat != Seat::Solitaire || status != SessionStatus::Active {
                    return Err(StoreError::Conflict(
                        "this solitaire session cannot accept moves",
                    ));
                }
                game.apply(action)
                    .map_err(|error| StoreError::InvalidAction(error.to_string()))?;
                if game.won {
                    SessionStatus::Complete
                } else {
                    status
                }
            }
            (GameState::Checkers(game), GameAction::Checkers(action)) => {
                if status != SessionStatus::Active {
                    return Err(StoreError::Conflict("waiting sessions cannot accept moves"));
                }
                let side = match seat {
                    Seat::Red => Side::Red,
                    Seat::Black => Side::Black,
                    Seat::Solitaire => {
                        return Err(StoreError::Conflict(
                            "this player does not have a checkers seat",
                        ));
                    }
                };
                game.apply_move(side, action)
                    .map_err(|error| StoreError::InvalidAction(error.to_string()))?;
                if game.winner.is_some() {
                    SessionStatus::Complete
                } else {
                    status
                }
            }
            _ => {
                return Err(StoreError::CorruptData(
                    "state and action variants do not agree",
                ));
            }
        };

        let next_version = row
            .state_version
            .checked_add(1)
            .ok_or(StoreError::CorruptData("state version overflow"))?;
        sqlx::query(
            "UPDATE game_sessions \
             SET state = $2, state_version = $3, status = $4::session_status, updated_at = NOW() \
             WHERE id = $1",
        )
        .bind(id)
        .bind(Json(serde_json::to_value(&state)?))
        .bind(next_version)
        .bind(next_status.as_db())
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO game_events (session_id, state_version, participant_id, action) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(next_version)
        .bind(row.participant_id)
        .bind(Json(serde_json::to_value(action)?))
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        self.load_authorized(id, access_token).await
    }

    async fn view_from_authorized_row(
        &self,
        row: AuthorizedSessionRow,
    ) -> Result<SessionView, StoreError> {
        let game_type = parse_game_type(&row.game_type)?;
        let state: GameState = serde_json::from_value(row.state.0)?;
        if state.game_type() != game_type {
            return Err(StoreError::CorruptData(
                "stored game type and state do not agree",
            ));
        }
        let you = Participant {
            id: row.participant_id,
            seat: Seat::parse(&row.seat)?,
            display_name: row.display_name,
        };
        let participants = self.participants(row.id).await?;
        Ok(SessionView {
            id: row.id,
            game_type,
            state,
            state_version: row.state_version,
            status: SessionStatus::parse(&row.status)?,
            you,
            participants,
        })
    }

    async fn participants(&self, session_id: Uuid) -> Result<Vec<Participant>, StoreError> {
        let rows = sqlx::query_as::<_, ParticipantRow>(
            "SELECT id, seat, display_name FROM session_participants \
             WHERE session_id = $1 ORDER BY seat",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(Participant {
                    id: row.id,
                    seat: Seat::parse(&row.seat)?,
                    display_name: row.display_name,
                })
            })
            .collect()
    }
}

#[derive(Debug, FromRow)]
struct SessionMetadataRow {
    game_type: String,
    status: String,
}

#[derive(Debug, FromRow)]
struct AuthorizedSessionRow {
    id: Uuid,
    game_type: String,
    state: Json<Value>,
    state_version: i64,
    status: String,
    participant_id: Uuid,
    seat: String,
    display_name: String,
}

#[derive(Debug, FromRow)]
struct ParticipantRow {
    id: Uuid,
    seat: String,
    display_name: String,
}

fn parse_game_type(value: &str) -> Result<GameType, StoreError> {
    GameType::parse(value).ok_or(StoreError::CorruptData("unknown game type"))
}

fn new_access_token() -> (String, Vec<u8>) {
    let mut bytes = [0_u8; 32];
    rand::rng().fill(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_access_token(&token);
    (token, hash)
}

fn hash_access_token(access_token: &str) -> Vec<u8> {
    Sha256::digest(access_token.as_bytes()).to_vec()
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("migration error")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("state serialization error")]
    Serialization(#[from] serde_json::Error),
    #[error("session not found")]
    NotFound,
    #[error("session access is unauthorized")]
    Unauthorized,
    #[error("session conflict: {0}")]
    Conflict(&'static str),
    #[error("invalid game action: {0}")]
    InvalidAction(String),
    #[error("corrupt session data: {0}")]
    CorruptData(&'static str),
}
