use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicTacToeState {
    pub board: Vec<u8>,
    pub turn: u8,
    pub winner: Option<u8>,
    pub draw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TicTacToeAction {
    Place { square: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("the game is complete")]
    GameFinished,
    #[error("invalid player")]
    InvalidPlayer,
    #[error("it is not your turn")]
    OutOfTurn,
    #[error("choose an empty square from 0 to 8")]
    InvalidSquare,
    #[error("invalid game state")]
    InvalidState,
}

impl TicTacToeState {
    pub fn initial() -> Self {
        Self {
            board: vec![0; 9],
            turn: 0,
            winner: None,
            draw: false,
        }
    }

    pub fn apply(&mut self, player: u8, action: &TicTacToeAction) -> Result<(), RuleError> {
        if self.is_complete() {
            return Err(RuleError::GameFinished);
        }
        if player > 1 {
            return Err(RuleError::InvalidPlayer);
        }
        if player != self.turn {
            return Err(RuleError::OutOfTurn);
        }
        if self.board.len() != 9 || self.board.iter().any(|&piece| piece > 2) {
            return Err(RuleError::InvalidState);
        }
        let TicTacToeAction::Place { square } = *action;
        if self.board.get(usize::from(square)) != Some(&0) {
            return Err(RuleError::InvalidSquare);
        }
        self.board[usize::from(square)] = player + 1;
        const LINES: [[usize; 3]; 8] = [
            [0, 1, 2],
            [3, 4, 5],
            [6, 7, 8],
            [0, 3, 6],
            [1, 4, 7],
            [2, 5, 8],
            [0, 4, 8],
            [2, 4, 6],
        ];
        if LINES
            .iter()
            .any(|line| line.iter().all(|&index| self.board[index] == player + 1))
        {
            self.winner = Some(player);
        } else if !self.board.contains(&0) {
            self.draw = true;
        }
        self.turn = 1 - player;
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.winner.is_some() || self.draw
    }

    pub fn legal_moves(&self) -> Vec<u8> {
        if self.is_complete() {
            return Vec::new();
        }
        self.board
            .iter()
            .enumerate()
            .filter_map(|(square, &piece)| (piece == 0).then_some(square as u8))
            .collect()
    }

    pub fn view_for(&self, _player: u8) -> serde_json::Value {
        serde_json::json!({ "board": self.board, "turn": self.turn, "winner": self.winner, "draw": self.draw, "legal_moves": self.legal_moves(), "rows": 3, "columns": 3 })
    }
}
