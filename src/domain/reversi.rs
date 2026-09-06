use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReversiState {
    pub board: Vec<u8>,
    pub turn: u8,
    pub winner: Option<u8>,
    pub draw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReversiAction {
    Place { square: u8 },
    Pass,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("the game is complete")]
    GameFinished,
    #[error("invalid player")]
    InvalidPlayer,
    #[error("it is not your turn")]
    OutOfTurn,
    #[error("a placement must enclose at least one opposing disc")]
    InvalidSquare,
    #[error("passing is only allowed when you have no legal placement")]
    CannotPass,
    #[error("invalid game state")]
    InvalidState,
}

impl ReversiState {
    pub fn initial() -> Self {
        let mut board = vec![0; 64];
        board[27] = 2;
        board[28] = 1;
        board[35] = 1;
        board[36] = 2;
        Self {
            board,
            turn: 0,
            winner: None,
            draw: false,
        }
    }

    pub fn apply(&mut self, player: u8, action: &ReversiAction) -> Result<(), RuleError> {
        if self.is_complete() {
            return Err(RuleError::GameFinished);
        }
        if player > 1 {
            return Err(RuleError::InvalidPlayer);
        }
        if player != self.turn {
            return Err(RuleError::OutOfTurn);
        }
        if self.board.len() != 64 || self.board.iter().any(|&piece| piece > 2) {
            return Err(RuleError::InvalidState);
        }
        match *action {
            ReversiAction::Place { square } => {
                let captured = self.captures(player, square);
                if captured.is_empty() {
                    return Err(RuleError::InvalidSquare);
                }
                self.board[usize::from(square)] = player + 1;
                for square in captured {
                    self.board[square] = player + 1;
                }
            }
            ReversiAction::Pass => {
                if !self.legal_moves_for(player).is_empty() {
                    return Err(RuleError::CannotPass);
                }
            }
        }
        self.turn = 1 - player;
        if self.legal_moves_for(0).is_empty() && self.legal_moves_for(1).is_empty() {
            let first = self.board.iter().filter(|&&piece| piece == 1).count();
            let second = self.board.iter().filter(|&&piece| piece == 2).count();
            match first.cmp(&second) {
                std::cmp::Ordering::Greater => self.winner = Some(0),
                std::cmp::Ordering::Less => self.winner = Some(1),
                std::cmp::Ordering::Equal => self.draw = true,
            }
        }
        Ok(())
    }

    fn captures(&self, player: u8, square: u8) -> Vec<usize> {
        if player > 1 || self.board.len() != 64 || self.board.get(usize::from(square)) != Some(&0) {
            return Vec::new();
        }
        let (row, column) = (i16::from(square / 8), i16::from(square % 8));
        let mut captured = Vec::new();
        for (dr, dc) in [
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, -1),
            (1, 0),
            (1, 1),
        ] {
            let (mut r, mut c) = (row + dr, column + dc);
            let start = captured.len();
            while (0..8).contains(&r)
                && (0..8).contains(&c)
                && self.board[(r * 8 + c) as usize] == 2 - player
            {
                captured.push((r * 8 + c) as usize);
                r += dr;
                c += dc;
            }
            if !(0..8).contains(&r)
                || !(0..8).contains(&c)
                || self.board[(r * 8 + c) as usize] != player + 1
            {
                captured.truncate(start);
            }
        }
        captured
    }

    pub fn is_complete(&self) -> bool {
        self.winner.is_some() || self.draw
    }

    pub fn legal_moves_for(&self, player: u8) -> Vec<u8> {
        if self.is_complete() {
            return Vec::new();
        }
        (0..64)
            .filter(|&square| !self.captures(player, square).is_empty())
            .collect()
    }

    pub fn view_for(&self, _player: u8) -> serde_json::Value {
        let legal_moves = self.legal_moves_for(self.turn);
        serde_json::json!({ "board": self.board, "turn": self.turn, "winner": self.winner, "draw": self.draw, "can_pass": !self.is_complete() && legal_moves.is_empty(), "legal_moves": legal_moves, "rows": 8, "columns": 8 })
    }
}
