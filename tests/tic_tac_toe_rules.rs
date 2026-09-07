use board_games::domain::tic_tac_toe::{RuleError, TicTacToeAction, TicTacToeState};

fn play(state: &mut TicTacToeState, square: u8) {
    state
        .apply(state.turn, &TicTacToeAction::Place { square })
        .unwrap();
}

#[test]
fn opening_turn_validation_and_rejections_are_atomic() {
    let mut state = TicTacToeState::initial();
    assert_eq!(state.legal_moves(), (0..9).collect::<Vec<_>>());
    for (player, square, error) in [
        (1, 0, RuleError::OutOfTurn),
        (2, 0, RuleError::InvalidPlayer),
        (0, 9, RuleError::InvalidSquare),
        (0, 255, RuleError::InvalidSquare),
    ] {
        let before = state.clone();
        assert_eq!(
            state.apply(player, &TicTacToeAction::Place { square }),
            Err(error)
        );
        assert_eq!(state, before);
    }
    play(&mut state, 0);
    assert_eq!(state.board[0], 1);
    let before = state.clone();
    assert_eq!(
        state.apply(1, &TicTacToeAction::Place { square: 0 }),
        Err(RuleError::InvalidSquare)
    );
    assert_eq!(state, before);
}

#[test]
fn detects_every_winning_line_and_rejects_postgame_actions() {
    for line in [
        [0, 1, 2],
        [3, 4, 5],
        [6, 7, 8],
        [0, 3, 6],
        [1, 4, 7],
        [2, 5, 8],
        [0, 4, 8],
        [2, 4, 6],
    ] {
        for player in 0..2 {
            let mut state = TicTacToeState::initial();
            state.turn = player;
            state.board[line[0]] = player + 1;
            state.board[line[1]] = player + 1;
            play(&mut state, line[2] as u8);
            assert_eq!(state.winner, Some(player));
            assert!(!state.draw);
            assert!(state.legal_moves().is_empty());
            let before = state.clone();
            assert_eq!(
                state.apply(state.turn, &TicTacToeAction::Place { square: 8 }),
                Err(RuleError::GameFinished)
            );
            assert_eq!(state, before);
        }
    }
}

#[test]
fn full_board_draw_and_json_roundtrip() {
    let mut state = TicTacToeState::initial();
    for square in [0, 1, 2, 4, 3, 5, 7, 6, 8] {
        play(&mut state, square);
    }
    assert!(state.draw);
    assert_eq!(state.winner, None);
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(
        serde_json::from_value::<TicTacToeState>(json).unwrap(),
        state
    );
    assert_eq!(state.view_for(0), state.view_for(1));
    assert_eq!(
        serde_json::to_value(TicTacToeAction::Place { square: 0 }).unwrap(),
        serde_json::json!({"kind":"place","square":0})
    );
}

#[test]
fn a_winning_last_square_is_a_win_not_a_draw() {
    let mut state = TicTacToeState {
        board: vec![1, 2, 2, 2, 1, 1, 1, 2, 0],
        turn: 0,
        winner: None,
        draw: false,
    };
    play(&mut state, 8);
    assert_eq!(state.winner, Some(0));
    assert!(!state.draw);
}
