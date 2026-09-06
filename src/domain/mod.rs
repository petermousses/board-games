pub mod battleship;
pub mod checkers;
pub mod chess;
pub mod clue;
pub mod connect_four;
pub mod reversi;
pub mod solitaire;
pub mod tic_tac_toe;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use self::{
    battleship::BattleshipState, checkers::CheckersState, chess::ChessState, clue::ClueState,
    connect_four::ConnectFourState, reversi::ReversiState, solitaire::SolitaireState,
    tic_tac_toe::TicTacToeState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameType {
    Solitaire,
    Checkers,
    Chess,
    Battleship,
    Clue,
    ConnectFour,
    Reversi,
    TicTacToe,
}

impl GameType {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Solitaire => "solitaire",
            Self::Checkers => "checkers",
            Self::Chess => "chess",
            Self::Battleship => "battleship",
            Self::Clue => "clue",
            Self::ConnectFour => "connect_four",
            Self::Reversi => "reversi",
            Self::TicTacToe => "tic_tac_toe",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "solitaire" => Some(Self::Solitaire),
            "checkers" => Some(Self::Checkers),
            "chess" => Some(Self::Chess),
            "battleship" => Some(Self::Battleship),
            "clue" => Some(Self::Clue),
            "connect_four" => Some(Self::ConnectFour),
            "reversi" => Some(Self::Reversi),
            "tic_tac_toe" => Some(Self::TicTacToe),
            _ => None,
        }
    }

    pub fn max_players(self) -> u8 {
        match self {
            Self::Solitaire => 1,
            Self::Clue => 6,
            _ => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "game_type", content = "state", rename_all = "snake_case")]
pub enum GameState {
    Solitaire(Box<SolitaireState>),
    Checkers(CheckersState),
    Chess(ChessState),
    Battleship(BattleshipState),
    Clue(ClueState),
    ConnectFour(ConnectFourState),
    Reversi(ReversiState),
    TicTacToe(TicTacToeState),
}

impl GameState {
    pub fn new(game_type: GameType, seed: u64) -> Self {
        match game_type {
            GameType::Solitaire => Self::Solitaire(Box::new(SolitaireState::seeded(seed))),
            GameType::Checkers => Self::Checkers(CheckersState::initial()),
            GameType::Chess => Self::Chess(ChessState::initial()),
            GameType::Battleship => Self::Battleship(BattleshipState::initial()),
            GameType::Clue => Self::Clue(ClueState::seeded(seed)),
            GameType::ConnectFour => Self::ConnectFour(ConnectFourState::initial()),
            GameType::Reversi => Self::Reversi(ReversiState::initial()),
            GameType::TicTacToe => Self::TicTacToe(TicTacToeState::initial()),
        }
    }

    pub fn game_type(&self) -> GameType {
        match self {
            Self::Solitaire(_) => GameType::Solitaire,
            Self::Checkers(_) => GameType::Checkers,
            Self::Chess(_) => GameType::Chess,
            Self::Battleship(_) => GameType::Battleship,
            Self::Clue(_) => GameType::Clue,
            Self::ConnectFour(_) => GameType::ConnectFour,
            Self::Reversi(_) => GameType::Reversi,
            Self::TicTacToe(_) => GameType::TicTacToe,
        }
    }

    /// API responses must use this projection; persisted GameState contains secrets.
    pub fn view_for(&self, player: u8) -> Value {
        let state = match self {
            Self::Solitaire(game) => json!(game),
            Self::Checkers(game) => json!(game),
            Self::Chess(game) => game.view_for(player),
            Self::Battleship(game) => game.view_for(player),
            Self::Clue(game) => game.view_for(player),
            Self::ConnectFour(game) => game.view_for(player),
            Self::Reversi(game) => game.view_for(player),
            Self::TicTacToe(game) => game.view_for(player),
        };
        json!({ "game_type": self.game_type(), "state": state })
    }

    pub fn is_complete(&self) -> bool {
        match self {
            Self::Solitaire(game) => game.won,
            Self::Checkers(game) => game.winner.is_some(),
            Self::Chess(game) => game.is_complete(),
            Self::Battleship(game) => game.is_complete(),
            Self::Clue(game) => game.is_complete(),
            Self::ConnectFour(game) => game.is_complete(),
            Self::Reversi(game) => game.is_complete(),
            Self::TicTacToe(game) => game.is_complete(),
        }
    }

    pub fn apply(&mut self, player: u8, action: &GameAction) -> Result<(), String> {
        match (self, action) {
            (Self::Solitaire(game), GameAction::Solitaire(action)) if player == 0 => {
                game.apply(action).map_err(|error| error.to_string())
            }
            (Self::Checkers(game), GameAction::Checkers(action)) if player < 2 => {
                let side = if player == 0 {
                    checkers::Side::Red
                } else {
                    checkers::Side::Black
                };
                game.apply_move(side, action)
                    .map_err(|error| error.to_string())
            }
            (Self::Chess(game), GameAction::Chess(action)) => game
                .apply(player, action)
                .map_err(|error| error.to_string()),
            (Self::Battleship(game), GameAction::Battleship(action)) => game
                .apply(player, action)
                .map_err(|error| error.to_string()),
            (Self::Clue(game), GameAction::Clue(action)) => game
                .apply(player, action)
                .map_err(|error| error.to_string()),
            (Self::ConnectFour(game), GameAction::ConnectFour(action)) => game
                .apply(player, action)
                .map_err(|error| error.to_string()),
            (Self::Reversi(game), GameAction::Reversi(action)) => game
                .apply(player, action)
                .map_err(|error| error.to_string()),
            (Self::TicTacToe(game), GameAction::TicTacToe(action)) => game
                .apply(player, action)
                .map_err(|error| error.to_string()),
            _ => Err("state, action, and player do not agree".to_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "game_type", content = "action", rename_all = "snake_case")]
pub enum GameAction {
    Solitaire(solitaire::SolitaireAction),
    Checkers(checkers::CheckersMove),
    Chess(chess::ChessAction),
    Battleship(battleship::BattleshipAction),
    Clue(clue::ClueAction),
    ConnectFour(connect_four::ConnectFourAction),
    Reversi(reversi::ReversiAction),
    TicTacToe(tic_tac_toe::TicTacToeAction),
}

impl GameAction {
    pub fn game_type(&self) -> GameType {
        match self {
            Self::Solitaire(_) => GameType::Solitaire,
            Self::Checkers(_) => GameType::Checkers,
            Self::Chess(_) => GameType::Chess,
            Self::Battleship(_) => GameType::Battleship,
            Self::Clue(_) => GameType::Clue,
            Self::ConnectFour(_) => GameType::ConnectFour,
            Self::Reversi(_) => GameType::Reversi,
            Self::TicTacToe(_) => GameType::TicTacToe,
        }
    }
}
