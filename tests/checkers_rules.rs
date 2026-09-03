use board_games::domain::checkers::{
    BLACK_MAN, CheckersMove, CheckersState, RED_MAN, RuleError, Side,
};

#[test]
fn checkers_rejects_a_quiet_move_when_any_capture_is_available() {
    // Black at 9 can capture red at 13 by landing on 16, so 9 -> 14 is illegal.
    let mut board = [0; 32];
    board[9] = BLACK_MAN;
    board[13] = RED_MAN;
    let mut state = CheckersState::from_board(board, Side::Black);

    let result = state.apply_move(
        Side::Black,
        &CheckersMove {
            from: 9,
            path: vec![14],
        },
    );

    assert_eq!(result, Err(RuleError::MandatoryCapture));
}

#[test]
fn checkers_requires_the_full_multi_jump_in_one_action() {
    // 9 -> 16 captures 13, then 16 -> 25 must capture 21.
    let mut board = [0; 32];
    board[9] = BLACK_MAN;
    board[13] = RED_MAN;
    board[21] = RED_MAN;
    // Keep a legal red move after the captures so this verifies turn handoff,
    // rather than an immediate black win from removing the final red piece.
    board[4] = RED_MAN;
    let mut state = CheckersState::from_board(board, Side::Black);

    let incomplete = state.apply_move(
        Side::Black,
        &CheckersMove {
            from: 9,
            path: vec![16],
        },
    );
    assert_eq!(incomplete, Err(RuleError::MustContinueCapture));

    let completed = state.apply_move(
        Side::Black,
        &CheckersMove {
            from: 9,
            path: vec![16, 25],
        },
    );
    assert_eq!(completed, Ok(()));
    assert_eq!(state.board[25], BLACK_MAN);
    assert_eq!(state.board[13], 0);
    assert_eq!(state.board[21], 0);
    assert_eq!(state.side_to_move, Side::Red);
}
