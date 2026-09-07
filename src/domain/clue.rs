use std::collections::VecDeque;

use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SUSPECTS: [&str; 6] = [
    "Miss Scarlet",
    "Colonel Mustard",
    "Mrs. White",
    "Mr. Green",
    "Mrs. Peacock",
    "Professor Plum",
];
pub const WEAPONS: [&str; 6] = [
    "Candlestick",
    "Dagger",
    "Lead Pipe",
    "Revolver",
    "Rope",
    "Wrench",
];
pub const ROOMS: [&str; 9] = [
    "Kitchen",
    "Ballroom",
    "Conservatory",
    "Dining Room",
    "Billiard Room",
    "Library",
    "Lounge",
    "Hall",
    "Study",
];
const START_POSITIONS: [u8; 6] = [0, 2, 8, 6, 1, 7];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Lobby,
    Playing,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Suggestion {
    player: u8,
    suspect: u8,
    weapon: u8,
    room: u8,
    refuter: Option<u8>,
    status: String,
    /// Never serialize this struct directly into a player response.
    shown_card: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Accusation {
    player: u8,
    suspect: u8,
    weapon: u8,
    room: u8,
    correct: bool,
}

/// The original compact board uses nine rooms joined by twelve corridors.
/// This contains secrets for storage; HTTP responses must use `view_for`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClueState {
    phase: Phase,
    seed: Option<u64>,
    players: u8,
    turn: u8,
    positions: [u8; 6],
    hands: Vec<Vec<u8>>,
    /// Card IDs: suspect 0..5, weapon 6..11, room 12..20.
    solution: Option<[u8; 3]>,
    eliminated: Vec<bool>,
    moved: bool,
    suggested: bool,
    suggestion: Option<Suggestion>,
    last_accusation: Option<Accusation>,
    pending_refuter: Option<u8>,
    winner: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClueAction {
    Move { destination: u8 },
    Suggest { suspect: u8, weapon: u8 },
    Refute { card: u8 },
    Accuse { suspect: u8, weapon: u8, room: u8 },
    EndTurn,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("Clue requires three to six players")]
    InvalidPlayerCount,
    #[error("the game has already started")]
    AlreadyStarted,
    #[error("the host must start the game after three to six players join")]
    NotStarted,
    #[error("the player is not seated in this game")]
    InvalidPlayer,
    #[error("the game is already complete")]
    GameFinished,
    #[error("it is not this player's turn")]
    OutOfTurn,
    #[error("eliminated players may only refute suggestions")]
    Eliminated,
    #[error("wait for the selected player to refute this suggestion")]
    RefutationPending,
    #[error("only the first clockwise player with a matching card may refute")]
    InvalidRefuter,
    #[error("show a card in your hand that matches the suggestion")]
    InvalidRefutation,
    #[error("move once before suggesting, to a different node within two connected steps")]
    InvalidMove,
    #[error("suggest once per turn, from inside a room")]
    InvalidSuggestion,
    #[error("the suspect, weapon, or room is invalid")]
    InvalidCard,
}

impl ClueState {
    /// `seed` must be server-generated and secret. No hands are dealt in the lobby.
    pub fn seeded(seed: u64) -> Self {
        Self {
            phase: Phase::Lobby,
            seed: Some(seed),
            players: 0,
            turn: 0,
            positions: START_POSITIONS,
            hands: Vec::new(),
            solution: None,
            eliminated: Vec::new(),
            moved: false,
            suggested: false,
            suggestion: None,
            last_accusation: None,
            pending_refuter: None,
            winner: None,
        }
    }

    /// The caller must enforce host authorization and serialize start/join writes.
    pub fn start(&mut self, players: u8) -> Result<(), RuleError> {
        if self.phase != Phase::Lobby {
            return Err(RuleError::AlreadyStarted);
        }
        if !(3..=6).contains(&players) {
            return Err(RuleError::InvalidPlayerCount);
        }
        let mut rng = StdRng::seed_from_u64(self.seed.expect("lobby has a seed"));
        let solution = [
            rng.random_range(0..6),
            rng.random_range(6..12),
            rng.random_range(12..21),
        ];
        let mut deck: Vec<u8> = (0..21).filter(|card| !solution.contains(card)).collect();
        deck.shuffle(&mut rng);
        let mut hands = vec![Vec::new(); usize::from(players)];
        for (index, card) in deck.into_iter().enumerate() {
            hands[index % usize::from(players)].push(card);
        }
        for hand in &mut hands {
            hand.sort_unstable();
        }
        self.hands = hands;
        self.solution = Some(solution);
        self.seed = None;
        self.players = players;
        self.eliminated = vec![false; usize::from(players)];
        self.phase = Phase::Playing;
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.phase == Phase::Complete
    }

    pub fn apply(&mut self, player: u8, action: &ClueAction) -> Result<(), RuleError> {
        // Rejected actions must never partially move a token or reveal a card.
        let mut candidate = self.clone();
        candidate.apply_inner(player, action)?;
        *self = candidate;
        Ok(())
    }

    fn apply_inner(&mut self, player: u8, action: &ClueAction) -> Result<(), RuleError> {
        if self.phase == Phase::Lobby {
            return Err(RuleError::NotStarted);
        }
        if self.is_complete() {
            return Err(RuleError::GameFinished);
        }
        if player >= self.players {
            return Err(RuleError::InvalidPlayer);
        }
        if let ClueAction::Refute { card } = action {
            if self.pending_refuter != Some(player) {
                return Err(RuleError::InvalidRefuter);
            }
            let suggestion = self.suggestion.as_ref().ok_or(RuleError::InvalidRefuter)?;
            if !self.hands[usize::from(player)].contains(card)
                || !suggested_cards(suggestion).contains(card)
            {
                return Err(RuleError::InvalidRefutation);
            }
            let suggestion = self
                .suggestion
                .as_mut()
                .expect("refutation has a suggestion");
            suggestion.shown_card = Some(*card);
            suggestion.status = "refuted".to_owned();
            self.pending_refuter = None;
            return Ok(());
        }
        if self.pending_refuter.is_some() {
            return Err(RuleError::RefutationPending);
        }
        if self.eliminated[usize::from(player)] {
            return Err(RuleError::Eliminated);
        }
        if player != self.turn {
            return Err(RuleError::OutOfTurn);
        }
        match action {
            ClueAction::Move { destination } => {
                if !self.legal_destinations(player).contains(destination) {
                    return Err(RuleError::InvalidMove);
                }
                self.positions[usize::from(player)] = *destination;
                self.moved = true;
            }
            ClueAction::Suggest { suspect, weapon } => {
                if *suspect >= 6 || *weapon >= 6 {
                    return Err(RuleError::InvalidCard);
                }
                let room = self.positions[usize::from(player)];
                if self.suggested || room >= 9 {
                    return Err(RuleError::InvalidSuggestion);
                }
                self.positions[usize::from(*suspect)] = room;
                let cards = [*suspect, *weapon + 6, room + 12];
                let refuter = (1..self.players)
                    .map(|offset| (player + offset) % self.players)
                    .find(|seat| {
                        self.hands[usize::from(*seat)]
                            .iter()
                            .any(|card| cards.contains(card))
                    });
                self.suggestion = Some(Suggestion {
                    player,
                    suspect: *suspect,
                    weapon: *weapon,
                    room,
                    refuter,
                    status: if refuter.is_some() {
                        "awaiting_refutation"
                    } else {
                        "unrefuted"
                    }
                    .to_owned(),
                    shown_card: None,
                });
                self.pending_refuter = refuter;
                self.suggested = true;
            }
            ClueAction::Accuse {
                suspect,
                weapon,
                room,
            } => {
                if *suspect >= 6 || *weapon >= 6 || *room >= 9 {
                    return Err(RuleError::InvalidCard);
                }
                let correct = self.solution == Some([*suspect, *weapon + 6, *room + 12]);
                self.last_accusation = Some(Accusation {
                    player,
                    suspect: *suspect,
                    weapon: *weapon,
                    room: *room,
                    correct,
                });
                if correct {
                    self.winner = Some(player);
                    self.phase = Phase::Complete;
                } else {
                    self.eliminated[usize::from(player)] = true;
                    self.advance_turn();
                }
            }
            ClueAction::EndTurn => self.advance_turn(),
            ClueAction::Refute { .. } => unreachable!("refutation handled before turn checks"),
        }
        Ok(())
    }

    fn advance_turn(&mut self) {
        if let Some(next) = (1..=self.players)
            .map(|offset| (self.turn + offset) % self.players)
            .find(|seat| !self.eliminated[usize::from(*seat)])
        {
            self.turn = next;
            self.moved = false;
            self.suggested = false;
        } else {
            self.phase = Phase::Complete;
        }
    }

    fn can_act(&self, player: u8) -> bool {
        self.phase == Phase::Playing
            && player < self.players
            && player == self.turn
            && !self.eliminated[usize::from(player)]
            && self.pending_refuter.is_none()
    }

    fn legal_destinations(&self, player: u8) -> Vec<u8> {
        if !self.can_act(player) || self.moved || self.suggested {
            return Vec::new();
        }
        let nodes = board_nodes();
        let origin = self.positions[usize::from(player)];
        let mut distances = [u8::MAX; 21];
        distances[usize::from(origin)] = 0;
        let mut queue = VecDeque::from([origin]);
        while let Some(current) = queue.pop_front() {
            let distance = distances[usize::from(current)];
            if distance == 2 {
                continue;
            }
            for neighbor in &nodes[usize::from(current)].neighbors {
                if distances[usize::from(*neighbor)] == u8::MAX {
                    distances[usize::from(*neighbor)] = distance + 1;
                    queue.push_back(*neighbor);
                }
            }
        }
        distances
            .into_iter()
            .enumerate()
            .filter_map(|(node, distance)| (distance > 0 && distance <= 2).then_some(node as u8))
            .collect()
    }

    /// Only the asking player sees the shown card. Only the pending refuter sees
    /// their choices. No seed, solution, or other hand is present, even at game end.
    pub fn view_for(&self, player: u8) -> Value {
        let seated = player < self.players;
        let can_act = self.can_act(player);
        let destinations = self.legal_destinations(player);
        let suggestion = self.suggestion.as_ref().map(|s| {
            json!({
                "player": s.player, "suspect": s.suspect, "weapon": s.weapon, "room": s.room,
                "refuter": s.refuter, "status": s.status,
            })
        });
        let shown_card = self
            .suggestion
            .as_ref()
            .filter(|s| seated && s.player == player)
            .and_then(|s| s.shown_card);
        let refutable_cards: Vec<u8> = if seated && self.pending_refuter == Some(player) {
            self.suggestion
                .as_ref()
                .map(|s| {
                    self.hands[usize::from(player)]
                        .iter()
                        .copied()
                        .filter(|card| suggested_cards(s).contains(card))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let your_hand = if seated {
            self.hands[usize::from(player)].clone()
        } else {
            Vec::new()
        };
        let cards: Vec<Value> = SUSPECTS
            .iter()
            .enumerate()
            .map(|(i, name)| json!({"id": i, "name": name, "category": "suspect"}))
            .chain(
                WEAPONS
                    .iter()
                    .enumerate()
                    .map(|(i, name)| json!({"id": i + 6, "name": name, "category": "weapon"})),
            )
            .chain(
                ROOMS
                    .iter()
                    .enumerate()
                    .map(|(i, name)| json!({"id": i + 12, "name": name, "category": "room"})),
            )
            .collect();
        let rooms: Vec<Value> = ROOMS
            .iter()
            .enumerate()
            .map(|(id, name)| json!({"id": id, "name": name, "card": id + 12}))
            .collect();
        json!({
            "phase": self.phase, "players": self.players, "turn": self.turn,
            "positions": &self.positions[..usize::from(self.players)],
            "suspect_positions": self.positions,
            "nodes": board_nodes(), "rooms": rooms, "suspects": SUSPECTS, "weapons": WEAPONS, "cards": cards,
            "your_hand": your_hand, "legal_destinations": destinations,
            "can_move": !destinations.is_empty(),
            "can_suggest": can_act && !self.suggested && self.positions[usize::from(player)] < 9,
            "can_end_turn": can_act, "can_accuse": can_act,
            "suggestion": suggestion, "pending_refuter": self.pending_refuter,
            "refutable_cards": refutable_cards, "shown_card": shown_card,
            "eliminated": self.eliminated, "winner": self.winner, "last_accusation": self.last_accusation,
            "rules": [
                "Clue: compact house board for 3–6 players. This original board uses nine rooms and twelve corridors.",
                "Each turn, optionally move up to two connected edges once. There are no dice, blocked spaces, or secret passages; tokens can share a space.",
                "You may make one suggestion in your current room, before or after moving. Suggesting ends movement and pulls that suspect into your room.",
                "The first clockwise opponent holding any suggested card must choose one to show privately. Eliminated players still refute.",
                "After any refutation, accuse or end your turn. You can also accuse before suggesting. A wrong accusation eliminates you; a correct one wins.",
                "If everyone accuses incorrectly, the game ends without a winner. Keep your own deduction notes; cards use IDs 0–5 suspects, 6–11 weapons, 12–20 rooms."
            ]
        })
    }
}

fn suggested_cards(suggestion: &Suggestion) -> [u8; 3] {
    [
        suggestion.suspect,
        suggestion.weapon + 6,
        suggestion.room + 12,
    ]
}

#[derive(Serialize)]
struct Node {
    id: u8,
    name: String,
    kind: &'static str,
    x: u8,
    z: u8,
    neighbors: Vec<u8>,
}

fn board_nodes() -> Vec<Node> {
    let mut nodes: Vec<Node> = ROOMS
        .iter()
        .enumerate()
        .map(|(id, name)| Node {
            id: id as u8,
            name: (*name).to_owned(),
            kind: "room",
            x: (id % 3 * 2) as u8,
            z: (id / 3 * 2) as u8,
            neighbors: Vec::new(),
        })
        .collect();
    let edges = (0..3)
        .flat_map(|row| (0..2).map(move |col| (row * 3 + col, row * 3 + col + 1)))
        .chain((0..2).flat_map(|row| (0..3).map(move |col| (row * 3 + col, (row + 1) * 3 + col))));
    for (from, to) in edges {
        let id = nodes.len() as u8;
        let x = (nodes[from].x + nodes[to].x) / 2;
        let z = (nodes[from].z + nodes[to].z) / 2;
        let name = format!("{} — {} corridor", ROOMS[from], ROOMS[to]);
        nodes[from].neighbors.push(id);
        nodes[to].neighbors.push(id);
        nodes.push(Node {
            id,
            name,
            kind: "corridor",
            x,
            z,
            neighbors: vec![from as u8, to as u8],
        });
    }
    nodes
}
