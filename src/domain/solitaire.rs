use rand::{SeedableRng, rngs::StdRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Card(pub u8);

impl Card {
    fn rank(self) -> u8 {
        self.0 % 13 + 1
    }

    fn suit(self) -> u8 {
        self.0 / 13
    }

    fn is_red(self) -> bool {
        matches!(self.suit(), 1 | 2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableauPile {
    pub cards: Vec<Card>,
    pub face_up_from: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolitaireState {
    pub stock: Vec<Card>,
    pub waste: Vec<Card>,
    pub foundations: [Vec<Card>; 4],
    pub tableau: [TableauPile; 7],
    pub moves: u32,
    pub won: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SolitaireAction {
    Draw,
    Move {
        from: PileRef,
        to: PileRef,
        count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PileRef {
    Waste,
    Tableau { index: u8 },
    Foundation { index: u8 },
}

impl SolitaireState {
    pub fn seeded(seed: u64) -> Self {
        let mut deck = (0..52).map(Card).collect::<Vec<_>>();
        deck.shuffle(&mut StdRng::seed_from_u64(seed));
        let mut tableau: [TableauPile; 7] = std::array::from_fn(|_| TableauPile {
            cards: Vec::new(),
            face_up_from: 0,
        });

        for (column, pile) in tableau.iter_mut().enumerate() {
            for _ in 0..=column {
                pile.cards
                    .push(deck.pop().expect("a standard deck has enough cards"));
            }
            pile.face_up_from = pile.cards.len() - 1;
        }

        Self {
            stock: deck,
            waste: Vec::new(),
            foundations: std::array::from_fn(|_| Vec::new()),
            tableau,
            moves: 0,
            won: false,
        }
    }

    pub fn apply(&mut self, action: &SolitaireAction) -> Result<(), RuleError> {
        if self.won {
            return Err(RuleError::GameFinished);
        }
        match action {
            SolitaireAction::Draw => self.draw(),
            SolitaireAction::Move { from, to, count } => self.move_cards(from, to, *count),
        }
    }

    fn draw(&mut self) -> Result<(), RuleError> {
        if let Some(card) = self.stock.pop() {
            self.waste.push(card);
        } else if self.waste.is_empty() {
            return Err(RuleError::StockEmpty);
        } else {
            self.stock = self.waste.drain(..).rev().collect();
        }
        self.moves = self.moves.saturating_add(1);
        Ok(())
    }

    fn move_cards(&mut self, from: &PileRef, to: &PileRef, count: usize) -> Result<(), RuleError> {
        if count == 0 || from == to {
            return Err(RuleError::IllegalMove);
        }
        let moving = self.source_cards(from, count)?;
        self.validate_destination(to, &moving)?;

        let moved = match from {
            PileRef::Waste => self.waste.split_off(self.waste.len() - count),
            PileRef::Foundation { index } => {
                let foundation = self.foundation_mut(*index)?;
                let split_at = foundation.len() - count;
                foundation.split_off(split_at)
            }
            PileRef::Tableau { index } => {
                let pile = self.tableau_mut(*index)?;
                let split_at = pile.cards.len() - count;
                let moved = pile.cards.split_off(split_at);
                if !pile.cards.is_empty() && pile.face_up_from == pile.cards.len() {
                    pile.face_up_from -= 1;
                }
                moved
            }
        };

        match to {
            PileRef::Tableau { index } => self.tableau_mut(*index)?.cards.extend(moved),
            PileRef::Foundation { index } => self.foundation_mut(*index)?.extend(moved),
            PileRef::Waste => return Err(RuleError::IllegalMove),
        }
        self.moves = self.moves.saturating_add(1);
        self.won = self.foundations.iter().map(Vec::len).sum::<usize>() == 52;
        Ok(())
    }

    fn source_cards(&self, source: &PileRef, count: usize) -> Result<Vec<Card>, RuleError> {
        let cards = match source {
            PileRef::Waste => {
                if count != 1 {
                    return Err(RuleError::IllegalMove);
                }
                &self.waste
            }
            PileRef::Foundation { index } => {
                if count != 1 {
                    return Err(RuleError::IllegalMove);
                }
                self.foundation(*index)?
            }
            PileRef::Tableau { index } => {
                let pile = self.tableau(*index)?;
                if count > pile.cards.len().saturating_sub(pile.face_up_from) {
                    return Err(RuleError::HiddenCard);
                }
                &pile.cards
            }
        };
        if cards.len() < count {
            return Err(RuleError::IllegalMove);
        }
        Ok(cards[cards.len() - count..].to_vec())
    }

    fn validate_destination(
        &self,
        destination: &PileRef,
        moving: &[Card],
    ) -> Result<(), RuleError> {
        let first = *moving.first().ok_or(RuleError::IllegalMove)?;
        match destination {
            PileRef::Waste => Err(RuleError::IllegalMove),
            PileRef::Foundation { index } => {
                if moving.len() != 1 || first.suit() != *index {
                    return Err(RuleError::IllegalMove);
                }
                let foundation = self.foundation(*index)?;
                let valid = foundation
                    .last()
                    .is_none_or(|top| top.rank() + 1 == first.rank());
                valid.then_some(()).ok_or(RuleError::IllegalMove)
            }
            PileRef::Tableau { index } => {
                let pile = self.tableau(*index)?;
                let valid = pile.cards.last().map_or(first.rank() == 13, |top| {
                    top.is_red() != first.is_red() && top.rank() == first.rank() + 1
                });
                valid.then_some(()).ok_or(RuleError::IllegalMove)
            }
        }
    }

    fn tableau(&self, index: u8) -> Result<&TableauPile, RuleError> {
        self.tableau
            .get(index as usize)
            .ok_or(RuleError::InvalidPile)
    }

    fn tableau_mut(&mut self, index: u8) -> Result<&mut TableauPile, RuleError> {
        self.tableau
            .get_mut(index as usize)
            .ok_or(RuleError::InvalidPile)
    }

    fn foundation(&self, index: u8) -> Result<&Vec<Card>, RuleError> {
        self.foundations
            .get(index as usize)
            .ok_or(RuleError::InvalidPile)
    }

    fn foundation_mut(&mut self, index: u8) -> Result<&mut Vec<Card>, RuleError> {
        self.foundations
            .get_mut(index as usize)
            .ok_or(RuleError::InvalidPile)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuleError {
    #[error("the game is already complete")]
    GameFinished,
    #[error("the stock and waste are empty")]
    StockEmpty,
    #[error("the move is not legal")]
    IllegalMove,
    #[error("a move cannot include a face-down card")]
    HiddenCard,
    #[error("the selected pile does not exist")]
    InvalidPile,
}
