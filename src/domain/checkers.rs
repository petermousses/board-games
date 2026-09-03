use serde::{Deserialize, Serialize};

pub const EMPTY: u8 = 0;
pub const RED_MAN: u8 = 1;
pub const RED_KING: u8 = 2;
pub const BLACK_MAN: u8 = 3;
pub const BLACK_KING: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Red,
    Black,
}

impl Side {
    pub fn opponent(self) -> Self {
        match self {
            Self::Red => Self::Black,
            Self::Black => Self::Red,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckersState {
    /// 32 playable squares, top-to-bottom and left-to-right over dark squares.
    pub board: [u8; 32],
    pub side_to_move: Side,
    pub winner: Option<Side>,
    pub turn: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckersMove {
    pub from: u8,
    /// Every landing square. A multi-jump is one atomic action.
    pub path: Vec<u8>,
}

impl CheckersState {
    pub fn initial() -> Self {
        let mut board = [EMPTY; 32];
        for piece in board.iter_mut().take(12) {
            *piece = BLACK_MAN;
        }
        for piece in board.iter_mut().skip(20) {
            *piece = RED_MAN;
        }
        Self {
            board,
            side_to_move: Side::Red,
            winner: None,
            turn: 0,
        }
    }

    pub fn from_board(board: [u8; 32], side_to_move: Side) -> Self {
        Self {
            board,
            side_to_move,
            winner: None,
            turn: 0,
        }
    }

    pub fn apply_move(&mut self, side: Side, action: &CheckersMove) -> Result<(), RuleError> {
        if self.winner.is_some() {
            return Err(RuleError::GameFinished);
        }
        if side != self.side_to_move {
            return Err(RuleError::OutOfTurn);
        }
        if action.path.is_empty() || action.from as usize >= self.board.len() {
            return Err(RuleError::InvalidPath);
        }

        let moving_piece = self.board[action.from as usize];
        if piece_side(moving_piece) != Some(side) {
            return Err(RuleError::InvalidSource);
        }

        let mandatory_capture = self.has_capture_for(side);
        let mut next_board = self.board;
        let mut current = action.from as usize;
        let mut captured_any = false;
        let mut promoted = false;

        for (step_index, destination) in action.path.iter().enumerate() {
            let destination = *destination as usize;
            if destination >= next_board.len() || next_board[destination] != EMPTY {
                return Err(RuleError::InvalidDestination);
            }

            let piece = next_board[current];
            let (from_row, from_column) = square_coordinates(current);
            let (to_row, to_column) = square_coordinates(destination);
            let row_delta = to_row - from_row;
            let column_delta = to_column - from_column;
            let absolute_row_delta = row_delta.unsigned_abs();
            let absolute_column_delta = column_delta.unsigned_abs();

            if absolute_row_delta != absolute_column_delta {
                return Err(RuleError::InvalidPath);
            }

            match absolute_row_delta {
                1 => {
                    if mandatory_capture {
                        return Err(RuleError::MandatoryCapture);
                    }
                    if captured_any
                        || action.path.len() != 1
                        || !moves_forward(piece, side, row_delta)
                    {
                        return Err(RuleError::InvalidPath);
                    }
                }
                2 => {
                    if !moves_forward(piece, side, row_delta) {
                        return Err(RuleError::InvalidPath);
                    }
                    let middle =
                        square_index(from_row + row_delta / 2, from_column + column_delta / 2)
                            .ok_or(RuleError::InvalidPath)?;
                    if piece_side(next_board[middle]) != Some(side.opponent()) {
                        return Err(RuleError::InvalidPath);
                    }
                    next_board[middle] = EMPTY;
                    captured_any = true;
                }
                _ => return Err(RuleError::InvalidPath),
            }

            next_board[destination] = piece;
            next_board[current] = EMPTY;
            current = destination;

            if is_man(piece) && reaches_promotion_row(piece, to_row) {
                next_board[current] = king_for(side);
                promoted = true;
                if step_index + 1 != action.path.len() {
                    return Err(RuleError::PromotionEndsTurn);
                }
            }
        }

        if mandatory_capture && !captured_any {
            return Err(RuleError::MandatoryCapture);
        }
        if captured_any && !promoted && has_capture_from(&next_board, current, side) {
            return Err(RuleError::MustContinueCapture);
        }

        self.board = next_board;
        self.turn = self.turn.saturating_add(1);
        let next_side = side.opponent();
        if !self.has_legal_move_for(next_side) {
            self.winner = Some(side);
        } else {
            self.side_to_move = next_side;
        }
        Ok(())
    }

    fn has_capture_for(&self, side: Side) -> bool {
        self.board.iter().enumerate().any(|(square, piece)| {
            piece_side(*piece) == Some(side) && has_capture_from(&self.board, square, side)
        })
    }

    fn has_legal_move_for(&self, side: Side) -> bool {
        if self.has_capture_for(side) {
            return true;
        }
        self.board.iter().enumerate().any(|(square, piece)| {
            piece_side(*piece) == Some(side) && has_quiet_move_from(&self.board, square, side)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("a capture is mandatory")]
    MandatoryCapture,
    #[error("a capturing sequence must continue")]
    MustContinueCapture,
    #[error("the game is already complete")]
    GameFinished,
    #[error("it is not this side's turn")]
    OutOfTurn,
    #[error("the selected square does not hold the moving side's piece")]
    InvalidSource,
    #[error("the destination square is invalid or occupied")]
    InvalidDestination,
    #[error("the move path is illegal")]
    InvalidPath,
    #[error("promotion ends the turn in American checkers")]
    PromotionEndsTurn,
}

fn piece_side(piece: u8) -> Option<Side> {
    match piece {
        RED_MAN | RED_KING => Some(Side::Red),
        BLACK_MAN | BLACK_KING => Some(Side::Black),
        _ => None,
    }
}

fn is_man(piece: u8) -> bool {
    matches!(piece, RED_MAN | BLACK_MAN)
}

fn king_for(side: Side) -> u8 {
    match side {
        Side::Red => RED_KING,
        Side::Black => BLACK_KING,
    }
}

fn reaches_promotion_row(piece: u8, row: i8) -> bool {
    matches!((piece, row), (RED_MAN, 0) | (BLACK_MAN, 7))
}

fn moves_forward(piece: u8, side: Side, row_delta: i8) -> bool {
    match piece {
        RED_KING | BLACK_KING => row_delta.unsigned_abs() <= 2,
        RED_MAN => side == Side::Red && row_delta < 0,
        BLACK_MAN => side == Side::Black && row_delta > 0,
        _ => false,
    }
}

fn has_capture_from(board: &[u8; 32], square: usize, side: Side) -> bool {
    let piece = board[square];
    let (row, column) = square_coordinates(square);
    directions_for(piece, side)
        .into_iter()
        .any(|(row_delta, column_delta)| {
            let Some(middle) = square_index(row + row_delta, column + column_delta) else {
                return false;
            };
            let Some(landing) = square_index(row + row_delta * 2, column + column_delta * 2) else {
                return false;
            };
            piece_side(board[middle]) == Some(side.opponent()) && board[landing] == EMPTY
        })
}

fn has_quiet_move_from(board: &[u8; 32], square: usize, side: Side) -> bool {
    let piece = board[square];
    let (row, column) = square_coordinates(square);
    directions_for(piece, side)
        .into_iter()
        .any(|(row_delta, column_delta)| {
            square_index(row + row_delta, column + column_delta)
                .is_some_and(|destination| board[destination] == EMPTY)
        })
}

fn directions_for(piece: u8, side: Side) -> Vec<(i8, i8)> {
    let row_directions: &[i8] = match piece {
        RED_KING | BLACK_KING => &[-1, 1],
        _ if side == Side::Red => &[-1],
        _ => &[1],
    };
    row_directions
        .iter()
        .flat_map(|row_delta| [(*row_delta, -1), (*row_delta, 1)])
        .collect()
}

fn square_coordinates(square: usize) -> (i8, i8) {
    let row = (square / 4) as i8;
    let column = (square % 4 * 2) as i8 + if row % 2 == 0 { 1 } else { 0 };
    (row, column)
}

fn square_index(row: i8, column: i8) -> Option<usize> {
    if !(0..8).contains(&row) || !(0..8).contains(&column) || (row + column) % 2 == 0 {
        return None;
    }
    Some(row as usize * 4 + column as usize / 2)
}
