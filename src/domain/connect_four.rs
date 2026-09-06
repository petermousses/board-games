use serde::{Deserialize, Serialize};

pub const ROWS: usize = 6;
pub const COLUMNS: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectFourState {
    /// Row-major, top-left first: 0 empty, 1 first player, 2 second player.
    pub board: Vec<u8>,
    pub turn: u8,
    pub winner: Option<u8>,
    pub draw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectFourAction {
    Drop { column: u8 },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("the game is complete")]
    GameFinished,
    #[error("invalid player")]
    InvalidPlayer,
    #[error("it is not your turn")]
    OutOfTurn,
    #[error("choose a non-full column from 0 to 6")]
    InvalidColumn,
    #[error("invalid game state")]
    InvalidState,
}

impl ConnectFourState {
    pub fn initial() -> Self {
        Self {
            board: vec![0; ROWS * COLUMNS],
            turn: 0,
            winner: None,
            draw: false,
        }
    }

    pub fn apply(&mut self, player: u8, action: &ConnectFourAction) -> Result<(), RuleError> {
        if self.is_complete() {
            return Err(RuleError::GameFinished);
        }
        if player > 1 {
            return Err(RuleError::InvalidPlayer);
        }
        if player != self.turn {
            return Err(RuleError::OutOfTurn);
        }
        if self.board.len() != ROWS * COLUMNS || self.board.iter().any(|&piece| piece > 2) {
            return Err(RuleError::InvalidState);
        }
        let ConnectFourAction::Drop { column } = *action;
        let column = usize::from(column);
        if column >= COLUMNS || self.board[column] != 0 {
            return Err(RuleError::InvalidColumn);
        }
        let row = (0..ROWS)
            .rev()
            .find(|&row| self.board[row * COLUMNS + column] == 0)
            .ok_or(RuleError::InvalidColumn)?;
        self.board[row * COLUMNS + column] = player + 1;
        if [(1, 0), (0, 1), (1, 1), (1, -1)].iter().any(|&(dr, dc)| {
            1 + self.run(row, column, dr, dc, player + 1)
                + self.run(row, column, -dr, -dc, player + 1)
                >= 4
        }) {
            self.winner = Some(player);
        } else if !self.board.contains(&0) {
            self.draw = true;
        }
        self.turn = 1 - player;
        Ok(())
    }

    fn run(&self, row: usize, column: usize, dr: isize, dc: isize, piece: u8) -> usize {
        let (mut row, mut column) = (row as isize + dr, column as isize + dc);
        let mut count = 0;
        while (0..ROWS as isize).contains(&row)
            && (0..COLUMNS as isize).contains(&column)
            && self.board[row as usize * COLUMNS + column as usize] == piece
        {
            count += 1;
            row += dr;
            column += dc;
        }
        count
    }

    pub fn is_complete(&self) -> bool {
        self.winner.is_some() || self.draw
    }

    pub fn legal_moves(&self) -> Vec<u8> {
        if self.is_complete() {
            return Vec::new();
        }
        (0..COLUMNS)
            .filter_map(|column| (self.board.get(column) == Some(&0)).then_some(column as u8))
            .collect()
    }

    pub fn view_for(&self, _player: u8) -> serde_json::Value {
        serde_json::json!({ "board": self.board, "turn": self.turn, "winner": self.winner, "draw": self.draw, "legal_moves": self.legal_moves(), "rows": ROWS, "columns": COLUMNS })
    }
}
