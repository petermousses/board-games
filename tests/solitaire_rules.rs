use board_games::domain::solitaire::{PileRef, SolitaireAction, SolitaireState};

#[test]
fn solitaire_deals_the_standard_hidden_tableau_shape() {
    let game = SolitaireState::seeded(7);

    assert_eq!(game.stock.len(), 24);
    assert!(game.waste.is_empty());
    for (index, pile) in game.tableau.iter().enumerate() {
        assert_eq!(pile.cards.len(), index + 1);
        assert_eq!(pile.face_up_from, index);
    }
}

#[test]
fn solitaire_draw_moves_one_card_from_stock_to_waste() {
    let mut game = SolitaireState::seeded(7);
    let expected = *game.stock.last().expect("seeded games have stock cards");

    game.apply(&SolitaireAction::Draw).expect("draw is legal");

    assert_eq!(game.stock.len(), 23);
    assert_eq!(game.waste, vec![expected]);
}

#[test]
fn solitaire_rejects_moving_more_than_the_visible_tableau_run() {
    let mut game = SolitaireState::seeded(7);

    let result = game.apply(&SolitaireAction::Move {
        from: PileRef::Tableau { index: 6 },
        to: PileRef::Tableau { index: 0 },
        count: 2,
    });

    assert!(result.is_err());
}
