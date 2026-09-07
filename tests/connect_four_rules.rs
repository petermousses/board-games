use board_games::domain::connect_four::{ConnectFourAction, ConnectFourState, RuleError};

fn drop(state: &mut ConnectFourState, column: u8) {
    state
        .apply(state.turn, &ConnectFourAction::Drop { column })
        .unwrap();
}

#[test]
fn gravity_and_full_columns() {
    let mut state = ConnectFourState::initial();
    for row in (0..6).rev() {
        let player = state.turn;
        drop(&mut state, 0);
        assert_eq!(state.board[row * 7], player + 1);
    }
    assert_eq!(state.legal_moves(), vec![1, 2, 3, 4, 5, 6]);
    let before = state.clone();
    assert_eq!(
        state.apply(0, &ConnectFourAction::Drop { column: 0 }),
        Err(RuleError::InvalidColumn)
    );
    assert_eq!(state, before);
}

#[test]
fn invalid_players_turns_and_columns_are_atomic() {
    let mut state = ConnectFourState::initial();
    for (player, column, error) in [
        (1, 0, RuleError::OutOfTurn),
        (2, 0, RuleError::InvalidPlayer),
        (0, 7, RuleError::InvalidColumn),
        (0, 255, RuleError::InvalidColumn),
    ] {
        let before = state.clone();
        assert_eq!(
            state.apply(player, &ConnectFourAction::Drop { column }),
            Err(error)
        );
        assert_eq!(state, before);
    }
}

#[test]
fn horizontal_vertical_and_both_diagonals_win() {
    for (stones, column) in [
        (vec![35, 36, 37], 3),
        (vec![35, 28, 21], 0),
        (vec![35, 29, 23], 3),
        (vec![38, 30, 22], 0),
    ] {
        for player in 0..2 {
            let mut state = ConnectFourState::initial();
            state.turn = player;
            for square in stones.iter().copied() {
                state.board[square] = player + 1;
            }
            let target_row = match stones.as_slice() {
                [35, 36, 37] => 5,
                [35, 28, 21] => 2,
                _ => 2,
            };
            for row in target_row + 1..6 {
                if state.board[row * 7 + column] == 0 {
                    state.board[row * 7 + column] = 2 - player;
                }
            }
            drop(&mut state, column as u8);
            assert_eq!(state.winner, Some(player), "stones {stones:?}");
            assert!(!state.draw);
            let before = state.clone();
            assert_eq!(
                state.apply(state.turn, &ConnectFourAction::Drop { column: 6 }),
                Err(RuleError::GameFinished)
            );
            assert_eq!(state, before);
            assert!(state.legal_moves().is_empty());
        }
    }
}

#[test]
fn no_false_win_across_row_boundary_and_draw_on_full_board() {
    let mut state = ConnectFourState::initial();
    state.board[33] = 1;
    state.board[34] = 1;
    state.board[35] = 1;
    drop(&mut state, 1);
    assert_eq!(state.winner, None);
    let mut draw = ConnectFourState {
        board: vec![
            1, 1, 2, 2, 1, 1, 0, 2, 2, 1, 1, 2, 2, 1, 1, 1, 2, 2, 1, 1, 2, 2, 2, 1, 1, 2, 2, 1, 1,
            1, 2, 2, 1, 1, 2, 2, 2, 1, 1, 2, 2, 1,
        ],
        turn: 1,
        winner: None,
        draw: false,
    };
    drop(&mut draw, 6);
    assert!(draw.draw);
    assert_eq!(draw.winner, None);
    assert_eq!(
        serde_json::from_value::<ConnectFourState>(serde_json::to_value(&draw).unwrap()).unwrap(),
        draw
    );
    assert_eq!(draw.view_for(0), draw.view_for(1));
}
