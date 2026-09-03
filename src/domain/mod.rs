pub mod checkers;
pub mod solitaire;

use serde::{Deserialize, Serialize};

use self::{checkers::CheckersState, solitaire::SolitaireState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameType {
    Solitaire,
    Checkers,
}

impl GameType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Solitaire => "solitaire",
            Self::Checkers => "checkers",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "solitaire" => Some(Self::Solitaire),
            "checkers" => Some(Self::Checkers),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "game_type", content = "state", rename_all = "snake_case")]
pub enum GameState {
    Solitaire(Box<SolitaireState>),
    Checkers(CheckersState),
}

impl GameState {
    pub fn new(game_type: GameType, seed: u64) -> Self {
        match game_type {
            GameType::Solitaire => Self::Solitaire(Box::new(SolitaireState::seeded(seed))),
            GameType::Checkers => Self::Checkers(CheckersState::initial()),
        }
    }

    pub fn game_type(&self) -> GameType {
        match self {
            Self::Solitaire(_) => GameType::Solitaire,
            Self::Checkers(_) => GameType::Checkers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "game_type", content = "action", rename_all = "snake_case")]
pub enum GameAction {
    Solitaire(solitaire::SolitaireAction),
    Checkers(checkers::CheckersMove),
}

impl GameAction {
    pub fn game_type(&self) -> GameType {
        match self {
            Self::Solitaire(_) => GameType::Solitaire,
            Self::Checkers(_) => GameType::Checkers,
        }
    }
}
