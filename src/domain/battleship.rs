use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const FLEET_LENGTHS: [u8; 5] = [5, 4, 3, 3, 2];
const BOARD_SIZE: usize = 10;
const CELLS: usize = BOARD_SIZE * BOARD_SIZE;

/// Contains both fleets. Persist this value, but send only `view_for` to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BattleshipState {
    fleets: [Vec<Vec<u8>>; 2],
    shots: [Vec<bool>; 2],
    ready: [bool; 2],
    turn: u8,
    winner: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipPlacement {
    pub row: u8,
    pub column: u8,
    pub horizontal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BattleshipAction {
    /// Ships must appear in the fixed order 5, 4, 3, 3, 2.
    PlaceFleet {
        ships: Vec<ShipPlacement>,
    },
    Fire {
        row: u8,
        column: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("the player is not seated in this game")]
    InvalidPlayer,
    #[error("the game is already complete")]
    GameFinished,
    #[error("this fleet is already locked in")]
    FleetAlreadyPlaced,
    #[error("place five non-overlapping ships of lengths 5, 4, 3, 3, 2 inside the board")]
    InvalidFleet,
    #[error("both players must place their fleets first")]
    NotReady,
    #[error("it is not this player's turn")]
    OutOfTurn,
    #[error("the target is outside the board")]
    InvalidTarget,
    #[error("this square has already been targeted")]
    AlreadyTargeted,
}

impl BattleshipState {
    pub fn initial() -> Self {
        Self {
            fleets: std::array::from_fn(|_| Vec::new()),
            shots: std::array::from_fn(|_| vec![false; CELLS]),
            ready: [false; 2],
            turn: 0,
            winner: None,
        }
    }

    pub fn is_complete(&self) -> bool {
        self.winner.is_some()
    }

    pub fn apply(&mut self, player: u8, action: &BattleshipAction) -> Result<(), RuleError> {
        if player >= 2 {
            return Err(RuleError::InvalidPlayer);
        }
        if self.is_complete() {
            return Err(RuleError::GameFinished);
        }
        let player = usize::from(player);
        match action {
            BattleshipAction::PlaceFleet { ships } => {
                if self.ready[player] {
                    return Err(RuleError::FleetAlreadyPlaced);
                }
                if ships.len() != FLEET_LENGTHS.len() {
                    return Err(RuleError::InvalidFleet);
                }
                let mut occupied = [false; CELLS];
                let mut fleet = Vec::with_capacity(FLEET_LENGTHS.len());
                for (placement, length) in ships.iter().zip(FLEET_LENGTHS) {
                    let mut ship = Vec::with_capacity(usize::from(length));
                    for offset in 0..usize::from(length) {
                        let row = usize::from(placement.row)
                            + if placement.horizontal { 0 } else { offset };
                        let column = usize::from(placement.column)
                            + if placement.horizontal { offset } else { 0 };
                        if row >= BOARD_SIZE || column >= BOARD_SIZE {
                            return Err(RuleError::InvalidFleet);
                        }
                        let square = row * BOARD_SIZE + column;
                        if occupied[square] {
                            return Err(RuleError::InvalidFleet);
                        }
                        occupied[square] = true;
                        ship.push(square as u8);
                    }
                    fleet.push(ship);
                }
                self.fleets[player] = fleet;
                self.ready[player] = true;
            }
            BattleshipAction::Fire { row, column } => {
                if !self.ready.iter().all(|ready| *ready) {
                    return Err(RuleError::NotReady);
                }
                if usize::from(self.turn) != player {
                    return Err(RuleError::OutOfTurn);
                }
                if usize::from(*row) >= BOARD_SIZE || usize::from(*column) >= BOARD_SIZE {
                    return Err(RuleError::InvalidTarget);
                }
                let target = usize::from(*row) * BOARD_SIZE + usize::from(*column);
                if self.shots[player][target] {
                    return Err(RuleError::AlreadyTargeted);
                }
                self.shots[player][target] = true;
                let opponent = 1 - player;
                if self.fleets[opponent]
                    .iter()
                    .flatten()
                    .all(|square| self.shots[player][usize::from(*square)])
                {
                    self.winner = Some(player as u8);
                } else {
                    self.turn = opponent as u8;
                }
            }
        }
        Ok(())
    }

    /// An explicit allowlist prevents unhit enemy cells from reaching the client.
    pub fn view_for(&self, player: u8) -> Value {
        let mut own_board = vec![0_u8; CELLS];
        let mut target_board = vec![0_u8; CELLS];
        if player < 2 {
            let player = usize::from(player);
            let opponent = 1 - player;
            for square in self.fleets[player].iter().flatten() {
                own_board[usize::from(*square)] = 1;
            }
            for (square, fired) in self.shots[opponent].iter().enumerate() {
                if *fired {
                    own_board[square] = if own_board[square] == 1 { 3 } else { 2 };
                }
            }
            for (square, fired) in self.shots[player].iter().enumerate() {
                if *fired {
                    target_board[square] = 2;
                }
            }
            for ship in &self.fleets[opponent] {
                let sunk = ship
                    .iter()
                    .all(|square| self.shots[player][usize::from(*square)]);
                for square in ship {
                    if self.shots[player][usize::from(*square)] {
                        target_board[usize::from(*square)] = if sunk { 4 } else { 3 };
                    }
                }
            }
        }
        json!({
            "phase": if self.is_complete() { "complete" }
                else if self.ready.iter().all(|ready| *ready) { "playing" }
                else { "setup" },
            "ready": self.ready,
            "turn": self.turn,
            "winner": self.winner,
            "own_board": own_board,
            "target_board": target_board,
            "fleet_lengths": FLEET_LENGTHS,
            "rules": [
                "Place ships in length order 5, 4, 3, 3, 2. Ships may touch but cannot overlap.",
                "Fleets lock when placed. Player 1 fires first; every shot passes the turn.",
                "Sink all five enemy ships to win."
            ]
        })
    }
}
