use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgConnection, PgPool, postgres::PgPoolOptions, types::Json};
use uuid::Uuid;

use crate::domain::{GameAction, GameState, GameType};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub id: Uuid,
    pub game_type: GameType,
    /// Redacted per-seat state. Never serialize the persistent GameState here.
    pub state: Value,
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
    pub player_index: u8,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    Solitaire,
    Red,
    Black,
    Player1,
    Player2,
    Player3,
    Player4,
    Player5,
    Player6,
}

impl Seat {
    fn as_db(self) -> &'static str {
        match self {
            Self::Solitaire => "solitaire",
            Self::Red => "red",
            Self::Black => "black",
            Self::Player1 => "player1",
            Self::Player2 => "player2",
            Self::Player3 => "player3",
            Self::Player4 => "player4",
            Self::Player5 => "player5",
            Self::Player6 => "player6",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "solitaire" => Ok(Self::Solitaire),
            "red" => Ok(Self::Red),
            "black" => Ok(Self::Black),
            "player1" => Ok(Self::Player1),
            "player2" => Ok(Self::Player2),
            "player3" => Ok(Self::Player3),
            "player4" => Ok(Self::Player4),
            "player5" => Ok(Self::Player5),
            "player6" => Ok(Self::Player6),
            _ => Err(StoreError::CorruptData("unknown participant seat")),
        }
    }

    fn for_player(game_type: GameType, player: u8) -> Result<Self, StoreError> {
        if player >= game_type.max_players() {
            return Err(StoreError::CorruptData("player exceeds game capacity"));
        }
        match game_type {
            GameType::Solitaire => Ok(Self::Solitaire),
            GameType::Checkers => Ok(if player == 0 { Self::Red } else { Self::Black }),
            _ => [
                Self::Player1,
                Self::Player2,
                Self::Player3,
                Self::Player4,
                Self::Player5,
                Self::Player6,
            ]
            .get(usize::from(player))
            .copied()
            .ok_or(StoreError::CorruptData("unknown player index")),
        }
    }

    fn player_index(self, game_type: GameType) -> Result<u8, StoreError> {
        let player = match self {
            Self::Solitaire | Self::Red | Self::Player1 => 0,
            Self::Black | Self::Player2 => 1,
            Self::Player3 => 2,
            Self::Player4 => 3,
            Self::Player5 => 4,
            Self::Player6 => 5,
        };
        if Self::for_player(game_type, player)? != self {
            return Err(StoreError::CorruptData("seat does not match game type"));
        }
        Ok(player)
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
        // Migrations across API replicas must be serialized before serving traffic.
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
        let seat = Seat::for_player(game_type, 0)?;
        let status = if game_type == GameType::Solitaire {
            SessionStatus::Active
        } else {
            SessionStatus::Lobby
        };
        let state = GameState::new(game_type, rand::random());
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO game_sessions (id, game_type, state, status) \
             VALUES ($1, $2::game_type, $3, $4::session_status)",
        )
        .bind(id)
        .bind(game_type.as_db())
        .bind(Json(serde_json::to_value(&state)?))
        .bind(status.as_db())
        .execute(&mut *transaction)
        .await?;
        insert_participant(
            &mut transaction,
            participant_id,
            id,
            seat,
            &display_name,
            token_hash,
        )
        .await?;
        transaction.commit().await?;

        let participant = Participant {
            id: participant_id,
            seat,
            player_index: 0,
            display_name,
        };
        Ok(SessionAccess {
            access_token,
            session: SessionView {
                id,
                game_type,
                state: state.view_for(0),
                state_version: 0,
                status,
                you: participant.clone(),
                participants: vec![participant],
            },
        })
    }

    pub async fn join_session(
        &self,
        id: Uuid,
        display_name: String,
    ) -> Result<SessionAccess, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let mut row = sqlx::query_as::<_, SessionRow>(
            "SELECT id, game_type::text AS game_type, state, state_version, status::text AS status \
             FROM game_sessions WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::NotFound)?;
        let (game_type, state, status) = decode_session(&row)?;
        if game_type == GameType::Solitaire {
            return Err(StoreError::Conflict("solitaire sessions are single player"));
        }
        if status != SessionStatus::Lobby {
            return Err(StoreError::Conflict(
                "this session is no longer accepting players",
            ));
        }
        let mut participants = participants(&mut transaction, id, game_type).await?;
        let player_index = u8::try_from(participants.len())
            .map_err(|_| StoreError::CorruptData("invalid participant count"))?;
        if player_index >= game_type.max_players() {
            return Err(StoreError::Conflict("this session is full"));
        }
        let seat = Seat::for_player(game_type, player_index)?;
        let participant_id = Uuid::new_v4();
        let (access_token, token_hash) = new_access_token();
        insert_participant(
            &mut transaction,
            participant_id,
            id,
            seat,
            &display_name,
            token_hash,
        )
        .await?;
        let you = Participant {
            id: participant_id,
            seat,
            player_index,
            display_name,
        };
        participants.push(you.clone());
        let next_status = if game_type == GameType::Clue {
            SessionStatus::Lobby
        } else {
            SessionStatus::Active
        };
        row.state_version = next_version(row.state_version)?;
        row.status = next_status.as_db().to_owned();
        persist_session(&mut transaction, &row, &state).await?;
        let view = session_view(&row, &state, you, participants)?;
        transaction.commit().await?;
        Ok(SessionAccess {
            access_token,
            session: view,
        })
    }

    pub async fn load_authorized(
        &self,
        id: Uuid,
        access_token: &str,
    ) -> Result<SessionView, StoreError> {
        let mut transaction = self.pool.begin().await?;
        // The shared row lock keeps state and membership from different queries consistent.
        let authorized = authorized_row(&mut transaction, id, access_token, false).await?;
        let (game_type, state, _) = decode_session(&authorized.session)?;
        let you = authorized.participant(game_type)?;
        let players = participants(&mut transaction, id, game_type).await?;
        let view = session_view(&authorized.session, &state, you, players)?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn start_session(
        &self,
        id: Uuid,
        access_token: &str,
    ) -> Result<SessionView, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let mut authorized = authorized_row(&mut transaction, id, access_token, true).await?;
        let (game_type, mut state, status) = decode_session(&authorized.session)?;
        let you = authorized.participant(game_type)?;
        if you.player_index != 0 {
            return Err(StoreError::Forbidden("only the host can start the game"));
        }
        if game_type != GameType::Clue || status != SessionStatus::Lobby {
            return Err(StoreError::Conflict("this session cannot be started"));
        }
        let players = participants(&mut transaction, id, game_type).await?;
        if !(3..=6).contains(&players.len()) {
            return Err(StoreError::Conflict("clue needs three to six players"));
        }
        let player_count = u8::try_from(players.len())
            .map_err(|_| StoreError::CorruptData("invalid participant count"))?;
        let GameState::Clue(game) = &mut state else {
            return Err(StoreError::CorruptData("clue session has the wrong state"));
        };
        game.start(player_count)
            .map_err(|error| StoreError::InvalidAction(error.to_string()))?;
        authorized.session.state_version = next_version(authorized.session.state_version)?;
        authorized.session.status = SessionStatus::Active.as_db().to_owned();
        persist_session(&mut transaction, &authorized.session, &state).await?;
        insert_event(
            &mut transaction,
            id,
            authorized.session.state_version,
            you.id,
            json!({ "game_type": "clue", "action": { "kind": "start", "players": player_count } }),
        )
        .await?;
        let view = session_view(&authorized.session, &state, you, players)?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn apply_action(
        &self,
        id: Uuid,
        access_token: &str,
        action: &GameAction,
        expected_version: Option<i64>,
    ) -> Result<SessionView, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let mut authorized = authorized_row(&mut transaction, id, access_token, true).await?;
        let (game_type, mut state, status) = decode_session(&authorized.session)?;
        let you = authorized.participant(game_type)?;
        if expected_version.is_some_and(|version| version != authorized.session.state_version) {
            return Err(StoreError::Conflict(
                "the board changed; refresh before trying again",
            ));
        }
        if action.game_type() != game_type {
            return Err(StoreError::InvalidAction(
                "action game type does not match the session".to_owned(),
            ));
        }
        if status != SessionStatus::Active {
            return Err(StoreError::Conflict(
                "only active sessions accept game actions",
            ));
        }
        state
            .apply(you.player_index, action)
            .map_err(StoreError::InvalidAction)?;
        authorized.session.state_version = next_version(authorized.session.state_version)?;
        authorized.session.status = if state.is_complete() {
            SessionStatus::Complete
        } else {
            SessionStatus::Active
        }
        .as_db()
        .to_owned();
        persist_session(&mut transaction, &authorized.session, &state).await?;
        insert_event(
            &mut transaction,
            id,
            authorized.session.state_version,
            you.id,
            serde_json::to_value(action)?,
        )
        .await?;
        let players = participants(&mut transaction, id, game_type).await?;
        let view = session_view(&authorized.session, &state, you, players)?;
        transaction.commit().await?;
        Ok(view)
    }
}

async fn insert_participant(
    connection: &mut PgConnection,
    participant_id: Uuid,
    session_id: Uuid,
    seat: Seat,
    display_name: &str,
    token_hash: Vec<u8>,
) -> Result<(), StoreError> {
    sqlx::query("INSERT INTO session_participants (id, session_id, seat, display_name, token_hash) VALUES ($1, $2, $3, $4, $5)")
        .bind(participant_id).bind(session_id).bind(seat.as_db()).bind(display_name).bind(token_hash)
        .execute(connection).await?;
    Ok(())
}

async fn persist_session(
    connection: &mut PgConnection,
    row: &SessionRow,
    state: &GameState,
) -> Result<(), StoreError> {
    sqlx::query("UPDATE game_sessions SET state = $2, state_version = $3, status = $4::session_status, updated_at = NOW() WHERE id = $1")
        .bind(row.id).bind(Json(serde_json::to_value(state)?)).bind(row.state_version).bind(&row.status)
        .execute(connection).await?;
    Ok(())
}

async fn insert_event(
    connection: &mut PgConnection,
    id: Uuid,
    version: i64,
    participant: Uuid,
    action: Value,
) -> Result<(), StoreError> {
    sqlx::query("INSERT INTO game_events (session_id, state_version, participant_id, action) VALUES ($1, $2, $3, $4)")
        .bind(id).bind(version).bind(participant).bind(Json(action)).execute(connection).await?;
    Ok(())
}

async fn authorized_row(
    connection: &mut PgConnection,
    id: Uuid,
    token: &str,
    exclusive: bool,
) -> Result<AuthorizedSessionRow, StoreError> {
    let query = if exclusive {
        "SELECT s.id, s.game_type::text AS game_type, s.state, s.state_version, s.status::text AS status, \
         p.id AS participant_id, p.seat, p.display_name FROM game_sessions s \
         INNER JOIN session_participants p ON p.session_id = s.id \
         WHERE s.id = $1 AND p.token_hash = $2 FOR UPDATE OF s"
    } else {
        "SELECT s.id, s.game_type::text AS game_type, s.state, s.state_version, s.status::text AS status, \
         p.id AS participant_id, p.seat, p.display_name FROM game_sessions s \
         INNER JOIN session_participants p ON p.session_id = s.id \
         WHERE s.id = $1 AND p.token_hash = $2 FOR SHARE OF s"
    };
    sqlx::query_as::<_, AuthorizedSessionRow>(query)
        .bind(id)
        .bind(hash_access_token(token))
        .fetch_optional(connection)
        .await?
        .ok_or(StoreError::Unauthorized)
}

async fn participants(
    connection: &mut PgConnection,
    id: Uuid,
    game_type: GameType,
) -> Result<Vec<Participant>, StoreError> {
    let rows = sqlx::query_as::<_, ParticipantRow>(
        "SELECT id, seat, display_name FROM session_participants WHERE session_id = $1",
    )
    .bind(id)
    .fetch_all(connection)
    .await?;
    let mut players = rows
        .into_iter()
        .map(|row| {
            let seat = Seat::parse(&row.seat)?;
            Ok(Participant {
                id: row.id,
                seat,
                player_index: seat.player_index(game_type)?,
                display_name: row.display_name,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    players.sort_by_key(|player| player.player_index);
    if players
        .iter()
        .enumerate()
        .any(|(index, player)| usize::from(player.player_index) != index)
    {
        return Err(StoreError::CorruptData(
            "participant seats are not contiguous",
        ));
    }
    Ok(players)
}

fn session_view(
    row: &SessionRow,
    state: &GameState,
    you: Participant,
    participants: Vec<Participant>,
) -> Result<SessionView, StoreError> {
    Ok(SessionView {
        id: row.id,
        game_type: state.game_type(),
        state: state.view_for(you.player_index),
        state_version: row.state_version,
        status: SessionStatus::parse(&row.status)?,
        you,
        participants,
    })
}

fn decode_session(row: &SessionRow) -> Result<(GameType, GameState, SessionStatus), StoreError> {
    let game_type =
        GameType::parse(&row.game_type).ok_or(StoreError::CorruptData("unknown game type"))?;
    let state: GameState = serde_json::from_value(row.state.0.clone())?;
    if state.game_type() != game_type {
        return Err(StoreError::CorruptData(
            "stored game type and state do not agree",
        ));
    }
    Ok((game_type, state, SessionStatus::parse(&row.status)?))
}

fn next_version(version: i64) -> Result<i64, StoreError> {
    version
        .checked_add(1)
        .ok_or(StoreError::CorruptData("state version overflow"))
}

#[derive(Debug, FromRow)]
struct SessionRow {
    id: Uuid,
    game_type: String,
    state: Json<Value>,
    state_version: i64,
    status: String,
}

#[derive(Debug, FromRow)]
struct AuthorizedSessionRow {
    #[sqlx(flatten)]
    session: SessionRow,
    participant_id: Uuid,
    seat: String,
    display_name: String,
}

impl AuthorizedSessionRow {
    fn participant(&self, game_type: GameType) -> Result<Participant, StoreError> {
        let seat = Seat::parse(&self.seat)?;
        Ok(Participant {
            id: self.participant_id,
            seat,
            player_index: seat.player_index(game_type)?,
            display_name: self.display_name.clone(),
        })
    }
}

#[derive(Debug, FromRow)]
struct ParticipantRow {
    id: Uuid,
    seat: String,
    display_name: String,
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
    #[error("session access is forbidden: {0}")]
    Forbidden(&'static str),
    #[error("session conflict: {0}")]
    Conflict(&'static str),
    #[error("invalid game action: {0}")]
    InvalidAction(String),
    #[error("corrupt session data: {0}")]
    CorruptData(&'static str),
}
