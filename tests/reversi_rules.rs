use board_games::domain::reversi::{ReversiAction, ReversiState, RuleError};

#[test]
fn legal_opening_flips_and_turns() {
    let mut state = ReversiState::initial();
    assert_eq!(state.legal_moves_for(0), vec![19, 26, 37, 44]);
    assert_eq!(state.legal_moves_for(1), vec![20, 29, 34, 43]);
    state
        .apply(0, &ReversiAction::Place { square: 19 })
        .unwrap();
    assert_eq!(state.board[19], 1);
    assert_eq!(state.board[27], 1);
    assert_eq!(state.board[36], 2);
    assert_eq!(state.turn, 1);
}

#[test]
fn invalid_players_turns_placements_and_pass_are_atomic() {
    let mut state = ReversiState::initial();
    for (player, action, error) in [
        (1, ReversiAction::Place { square: 20 }, RuleError::OutOfTurn),
        (
            2,
            ReversiAction::Place { square: 19 },
            RuleError::InvalidPlayer,
        ),
        (
            0,
            ReversiAction::Place { square: 27 },
            RuleError::InvalidSquare,
        ),
        (
            0,
            ReversiAction::Place { square: 255 },
            RuleError::InvalidSquare,
        ),
        (
            0,
            ReversiAction::Place { square: 0 },
            RuleError::InvalidSquare,
        ),
        (0, ReversiAction::Pass, RuleError::CannotPass),
    ] {
        let before = state.clone();
        assert_eq!(state.apply(player, &action), Err(error));
        assert_eq!(state, before);
    }
}

#[test]
fn captures_every_enclosed_direction() {
    let mut state = ReversiState::initial();
    state.board.fill(0);
    let mut adjacent = Vec::new();
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
        let near = ((3 + dr) * 8 + 3 + dc) as usize;
        let far = ((3 + 2 * dr) * 8 + 3 + 2 * dc) as usize;
        state.board[near] = 2;
        state.board[far] = 1;
        adjacent.push(near);
    }
    state
        .apply(0, &ReversiAction::Place { square: 27 })
        .unwrap();
    for square in adjacent {
        assert_eq!(state.board[square], 1);
    }
    assert_eq!(state.winner, Some(0));
}

#[test]
fn no_capture_across_board_edges() {
    let mut state = ReversiState::initial();
    state.board.fill(0);
    state.board[8] = 2;
    state.board[9] = 1;
    let before = state.clone();
    assert_eq!(
        state.apply(0, &ReversiAction::Place { square: 7 }),
        Err(RuleError::InvalidSquare)
    );
    assert_eq!(state, before);
}

#[test]
fn forced_pass_hands_over_and_final_disc_ends_game() {
    let mut state = ReversiState {
        board: vec![2; 64],
        turn: 0,
        winner: None,
        draw: false,
    };
    state.board[0] = 0;
    state.board[1] = 1;
    assert!(state.legal_moves_for(0).is_empty());
    assert_eq!(state.legal_moves_for(1), vec![0]);
    assert_eq!(state.view_for(0)["can_pass"], true);
    state.apply(0, &ReversiAction::Pass).unwrap();
    assert_eq!(state.turn, 1);
    assert!(!state.is_complete());
    state.apply(1, &ReversiAction::Place { square: 0 }).unwrap();
    assert_eq!(state.winner, Some(1));
    assert!(!state.draw);
    let before = state.clone();
    assert_eq!(
        state.apply(0, &ReversiAction::Pass),
        Err(RuleError::GameFinished)
    );
    assert_eq!(state, before);
}

#[test]
fn equal_counts_end_in_draw_even_with_empty_squares() {
    let mut state = ReversiState {
        board: vec![0; 64],
        turn: 0,
        winner: None,
        draw: false,
    };
    state.board[0] = 1;
    state.board[63] = 2;
    state.apply(0, &ReversiAction::Pass).unwrap();
    assert!(state.draw);
    assert_eq!(state.winner, None);
    assert_eq!(
        serde_json::from_value::<ReversiState>(serde_json::to_value(&state).unwrap()).unwrap(),
        state
    );
    assert_eq!(state.view_for(0), state.view_for(1));
}

#[test]
fn deterministic_complete_games_preserve_disc_count_and_terminate() {
    for choice in 0..4 {
        let mut state = ReversiState::initial();
        let mut turns = 0;
        while !state.is_complete() {
            let moves = state.legal_moves_for(state.turn);
            let previous_count = state.board.iter().filter(|&&disc| disc != 0).count();
            let previous_turn = state.turn;
            let action = if moves.is_empty() {
                ReversiAction::Pass
            } else {
                ReversiAction::Place {
                    square: moves[(choice + turns) % moves.len()],
                }
            };
            state.apply(previous_turn, &action).unwrap();
            let count = state.board.iter().filter(|&&disc| disc != 0).count();
            assert_eq!(
                count,
                previous_count + usize::from(!matches!(action, ReversiAction::Pass))
            );
            assert_eq!(state.turn, 1 - previous_turn);
            turns += 1;
            assert!(turns <= 120);
        }
        let first = state.board.iter().filter(|&&disc| disc == 1).count();
        let second = state.board.iter().filter(|&&disc| disc == 2).count();
        assert_eq!(state.draw, first == second);
        assert_eq!(
            state.winner,
            if first == second {
                None
            } else {
                Some(u8::from(second > first))
            }
        );
    }
}
