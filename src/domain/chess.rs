use cozy_chess::{
    Board, Color, Piece, Square,
    util::{display_uci_move, parse_uci_move},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChessState {
    /// Standard FEN, including the uncapped halfmove clock.
    pub fen: String,
    pub turn: u8,
    pub winner: Option<u8>,
    pub draw: bool,
    pub draw_reason: Option<String>,
    /// cozy-chess caps its clock at 100; keep our own for the 75-move rule.
    pub halfmove_clock: u16,
    /// Positions since the last pawn move/capture, including the current position.
    pub position_history: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChessAction {
    Move {
        from: String,
        to: String,
        promotion: Option<String>,
    },
    Resign,
    /// Claim a threefold repetition or 50-move draw in the current position.
    ClaimDraw,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("the game is complete")]
    GameFinished,
    #[error("invalid player")]
    InvalidPlayer,
    #[error("it is not your turn")]
    OutOfTurn,
    #[error("invalid move: use squares a1 to h8 and promotion q, r, b, or n")]
    InvalidMove,
    #[error("that move is not legal")]
    IllegalMove,
    #[error("the current position does not qualify for a draw claim")]
    InvalidDrawClaim,
    #[error("invalid chess position")]
    InvalidState,
}

impl ChessState {
    pub fn initial() -> Self {
        Self::from_fen(&Board::default().to_string()).expect("the standard start position is valid")
    }

    /// Creates a standard-chess position. FEN cannot recover earlier repetition history.
    pub fn from_fen(fen: &str) -> Result<Self, RuleError> {
        let (board, halfmove_clock) = parse_position(fen)?;
        let fen = fen_with_clock(&board, halfmove_clock);
        let mut state = Self {
            position_history: vec![fen.clone()],
            fen,
            turn: player_for(board.side_to_move()),
            winner: None,
            draw: false,
            draw_reason: None,
            halfmove_clock,
        };
        state.adjudicate(&board);
        Ok(state)
    }

    pub fn apply(&mut self, player: u8, action: &ChessAction) -> Result<(), RuleError> {
        if self.is_complete() {
            return Err(RuleError::GameFinished);
        }
        if player > 1 {
            return Err(RuleError::InvalidPlayer);
        }
        let (mut board, clock) = parse_position(&self.fen)?;
        if self.turn != player_for(board.side_to_move()) || clock != self.halfmove_clock {
            return Err(RuleError::InvalidState);
        }
        // A player may resign even while the opponent is thinking.
        if matches!(action, ChessAction::Resign) {
            let opponent = if player == 0 {
                Color::Black
            } else {
                Color::White
            };
            if board.colors(opponent).len() == 1 {
                self.set_draw("resignation_without_mating_material");
            } else {
                self.winner = Some(1 - player);
            }
            return Ok(());
        }
        if player != self.turn {
            return Err(RuleError::OutOfTurn);
        }
        match action {
            ChessAction::Move {
                from,
                to,
                promotion,
            } => {
                // Validate each field separately so concatenated malformed fields cannot
                // accidentally become a valid UCI move.
                let _from: Square = from.parse().map_err(|_| RuleError::InvalidMove)?;
                let _to: Square = to.parse().map_err(|_| RuleError::InvalidMove)?;
                if promotion
                    .as_deref()
                    .is_some_and(|p| !matches!(p, "q" | "r" | "b" | "n"))
                {
                    return Err(RuleError::InvalidMove);
                }
                let uci = format!("{from}{to}{}", promotion.as_deref().unwrap_or(""));
                let mv = parse_uci_move(&board, &uci).map_err(|_| RuleError::InvalidMove)?;
                // Reject the engine's king-captures-rook encoding at our UCI boundary.
                if display_uci_move(&board, mv).to_string() != uci {
                    return Err(RuleError::IllegalMove);
                }
                board.try_play(mv).map_err(|_| RuleError::IllegalMove)?;
                let halfmove_clock = if board.halfmove_clock() == 0 {
                    0
                } else {
                    self.halfmove_clock.saturating_add(1)
                };
                let mut next = self.clone();
                next.halfmove_clock = halfmove_clock;
                next.fen = fen_with_clock(&board, halfmove_clock);
                next.turn = player_for(board.side_to_move());
                if halfmove_clock == 0 {
                    next.position_history.clear();
                }
                next.position_history.push(next.fen.clone());
                next.adjudicate(&board);
                *self = next;
            }
            ChessAction::ClaimDraw => {
                let reason = if self.repetitions(&board) >= 3 {
                    "threefold_repetition"
                } else if self.halfmove_clock >= 100 {
                    "fifty_move_rule"
                } else {
                    return Err(RuleError::InvalidDrawClaim);
                };
                self.draw = true;
                self.draw_reason = Some(reason.into());
            }
            ChessAction::Resign => unreachable!("handled above"),
        }
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.winner.is_some() || self.draw
    }

    pub fn legal_moves(&self) -> Vec<String> {
        if self.is_complete() {
            return Vec::new();
        }
        parse_position(&self.fen)
            .map(|(board, _)| legal_moves(&board))
            .unwrap_or_default()
    }

    pub fn view_for(&self, _player: u8) -> serde_json::Value {
        let parsed = parse_position(&self.fen).ok().map(|(board, _)| board);
        let mut squares: Vec<Option<String>> = Vec::with_capacity(64);
        for rank in (0..8).rev() {
            for file in 0..8 {
                let square = Square::index(rank * 8 + file);
                let piece = parsed.as_ref().and_then(|board| {
                    board.piece_on(square).map(|piece| {
                        let letter: char = piece.into();
                        if board.color_on(square) == Some(Color::White) {
                            letter.to_ascii_uppercase().to_string()
                        } else {
                            letter.to_string()
                        }
                    })
                });
                squares.push(piece);
            }
        }
        let can_claim_draw = !self.is_complete()
            && parsed
                .as_ref()
                .is_some_and(|board| self.halfmove_clock >= 100 || self.repetitions(board) >= 3);
        serde_json::json!({
            "board": squares, "fen": self.fen, "turn": self.turn, "winner": self.winner,
            "draw": self.draw, "draw_reason": self.draw_reason,
            "in_check": parsed.as_ref().is_some_and(|board| !board.checkers().is_empty()),
            "legal_moves": self.legal_moves(), "can_claim_draw": can_claim_draw,
            "rows": 8, "columns": 8,
        })
    }

    fn repetitions(&self, board: &Board) -> usize {
        self.position_history
            .iter()
            .filter(|fen| parse_position(fen).is_ok_and(|(past, _)| board.same_position(&past)))
            .count()
    }

    fn adjudicate(&mut self, board: &Board) {
        // Checkmate takes precedence over automatic draws. Do not use Board::status:
        // it automatically draws at 100 halfmoves; FIDE's automatic threshold is 150.
        if !board.generate_moves(|_| true) {
            if board.checkers().is_empty() {
                self.set_draw("stalemate");
            } else {
                self.winner = Some(1 - self.turn);
            }
        } else if dead_material(board) {
            self.set_draw("insufficient_material");
        } else if self.halfmove_clock >= 150 {
            self.set_draw("seventy_five_move_rule");
        } else if self.repetitions(board) >= 5 {
            self.set_draw("fivefold_repetition");
        }
    }

    fn set_draw(&mut self, reason: &str) {
        self.draw = true;
        self.draw_reason = Some(reason.into());
    }
}

fn player_for(color: Color) -> u8 {
    if color == Color::White { 0 } else { 1 }
}

fn parse_position(fen: &str) -> Result<(Board, u16), RuleError> {
    let mut fields: Vec<_> = fen.split_whitespace().map(str::to_owned).collect();
    if fields.len() != 6 {
        return Err(RuleError::InvalidState);
    }
    let clock: u16 = fields[4].parse().map_err(|_| RuleError::InvalidState)?;
    fields[4] = clock.min(100).to_string();
    let board = Board::from_fen(&fields.join(" "), false).map_err(|_| RuleError::InvalidState)?;
    Ok((board, clock))
}

fn fen_with_clock(board: &Board, clock: u16) -> String {
    let mut fields: Vec<_> = board
        .to_string()
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    fields[4] = clock.to_string();
    fields.join(" ")
}

fn legal_moves(board: &Board) -> Vec<String> {
    let mut moves = Vec::new();
    board.generate_moves(|batch| {
        moves.extend(
            batch
                .into_iter()
                .map(|mv| display_uci_move(board, mv).to_string()),
        );
        false
    });
    moves.sort();
    moves
}

/// Conservative dead-position detection: bare kings, one minor piece, or only
/// bishops confined to one square color. Two knights are not automatically dead:
/// a cooperative checkmate is possible. Exotic blocked-material dead positions
/// are not inferred, avoiding false draws in otherwise playable positions.
fn dead_material(board: &Board) -> bool {
    if !(board.pieces(Piece::Pawn) | board.pieces(Piece::Rook) | board.pieces(Piece::Queen))
        .is_empty()
    {
        return false;
    }
    let minors = board.pieces(Piece::Bishop) | board.pieces(Piece::Knight);
    if minors.len() <= 1 {
        return true;
    }
    if !board.pieces(Piece::Knight).is_empty() {
        return false;
    }
    let mut color = None;
    for square in board.pieces(Piece::Bishop) {
        let next = (square.file() as usize + square.rank() as usize) % 2;
        if color.is_some_and(|previous| previous != next) {
            return false;
        }
        color = Some(next);
    }
    true
}
