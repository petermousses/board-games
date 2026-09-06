use board_games::domain::chess::{ChessAction, ChessState, RuleError};

fn action(uci: &str) -> ChessAction {
    ChessAction::Move {
        from: uci[..2].into(),
        to: uci[2..4].into(),
        promotion: uci.get(4..).filter(|s| !s.is_empty()).map(str::to_owned),
    }
}
fn play(state: &mut ChessState, uci: &str) {
    state.apply(state.turn, &action(uci)).unwrap();
}
fn piece(state: &ChessState, square: &str) -> serde_json::Value {
    let bytes = square.as_bytes();
    state.view_for(0)["board"][usize::from(b'8' - bytes[1]) * 8 + usize::from(bytes[0] - b'a')]
        .clone()
}
fn rejects(state: &mut ChessState, player: u8, move_action: ChessAction, error: RuleError) {
    let before = state.clone();
    assert_eq!(state.apply(player, &move_action), Err(error));
    assert_eq!(*state, before);
}

#[test]
fn start_position_and_move_contract() {
    let mut state = ChessState::initial();
    assert_eq!(state.legal_moves().len(), 20);
    assert_eq!(piece(&state, "a8"), "r");
    assert_eq!(piece(&state, "e1"), "K");
    assert_eq!(piece(&state, "e4"), serde_json::Value::Null);
    assert_eq!(state.view_for(0), state.view_for(1));
    play(&mut state, "e2e4");
    assert_eq!(state.turn, 1);
    assert_eq!(piece(&state, "e4"), "P");
    assert_eq!(piece(&state, "e2"), serde_json::Value::Null);
    assert_eq!(state.legal_moves().len(), 20);
    play(&mut state, "e7e5");
    assert_eq!(
        serde_json::from_value::<ChessState>(serde_json::to_value(&state).unwrap()).unwrap(),
        state
    );
    assert_eq!(
        serde_json::to_value(action("a7a8n")).unwrap(),
        serde_json::json!({"kind":"move","from":"a7","to":"a8","promotion":"n"})
    );
}

#[test]
fn illegal_moves_invalid_input_and_actor_rejections_are_atomic() {
    let mut state = ChessState::initial();
    rejects(&mut state, 1, action("e2e4"), RuleError::OutOfTurn);
    rejects(&mut state, 2, action("e2e4"), RuleError::InvalidPlayer);
    for uci in ["e2e5", "e1g1", "a1a4", "a3a4", "e7e5", "e2e4q"] {
        rejects(&mut state, 0, action(uci), RuleError::IllegalMove);
    }
    for (from, to, promotion) in [
        ("e", "2e4", None),
        ("E2", "e4", None),
        ("e2", "e9", None),
        ("e2", "e4", Some("k")),
        ("e2", "e4", Some("")),
    ] {
        rejects(
            &mut state,
            0,
            ChessAction::Move {
                from: from.into(),
                to: to.into(),
                promotion: promotion.map(str::to_owned),
            },
            RuleError::InvalidMove,
        );
    }
    rejects(
        &mut state,
        0,
        ChessAction::ClaimDraw,
        RuleError::InvalidDrawClaim,
    );
}

#[test]
fn pinned_pieces_and_check_cannot_expose_the_king() {
    let mut state = ChessState::from_fen("4r1k1/8/8/8/8/8/4R3/4K3 w - - 0 1").unwrap();
    rejects(&mut state, 0, action("e2f2"), RuleError::IllegalMove);
    play(&mut state, "e2e8");
    assert_eq!(piece(&state, "e8"), "R");
    let mut checked = ChessState::from_fen("4r1k1/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    assert_eq!(checked.view_for(0)["in_check"], true);
    rejects(&mut checked, 0, action("e1e2"), RuleError::IllegalMove);
    play(&mut checked, "e1f1");
    assert_eq!(checked.view_for(0)["in_check"], false);
}

#[test]
fn all_four_castles_use_standard_uci_and_relocate_both_pieces() {
    for (side, uci, king, rook) in [
        ("w", "e1g1", "g1", "f1"),
        ("w", "e1c1", "c1", "d1"),
        ("b", "e8g8", "g8", "f8"),
        ("b", "e8c8", "c8", "d8"),
    ] {
        let mut state =
            ChessState::from_fen(&format!("r3k2r/8/8/8/8/8/8/R3K2R {side} KQkq - 0 1")).unwrap();
        assert!(state.legal_moves().contains(&uci.to_owned()));
        assert!(!state.legal_moves().contains(&"e1h1".to_owned()));
        play(&mut state, uci);
        assert_eq!(piece(&state, king), if side == "w" { "K" } else { "k" });
        assert_eq!(piece(&state, rook), if side == "w" { "R" } else { "r" });
    }
    let mut state = ChessState::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    rejects(&mut state, 0, action("e1h1"), RuleError::IllegalMove);
}

#[test]
fn cannot_castle_through_check_out_of_check_or_after_rook_moves() {
    for fen in [
        "r3k2r/8/8/8/2b5/8/8/R3K2R w KQkq - 0 1",
        "r3k2r/8/8/8/8/8/4r3/R3K2R w KQkq - 0 1",
    ] {
        let mut state = ChessState::from_fen(fen).unwrap();
        rejects(&mut state, 0, action("e1g1"), RuleError::IllegalMove);
    }
    let mut state = ChessState::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    for uci in ["h1h2", "a8a7", "h2h1", "a7a8"] {
        play(&mut state, uci);
    }
    rejects(&mut state, 0, action("e1g1"), RuleError::IllegalMove);
    assert!(state.legal_moves().contains(&"e1c1".into()));
}

#[test]
fn en_passant_is_immediate_and_must_preserve_king_safety() {
    let mut state = ChessState::initial();
    for uci in ["e2e4", "a7a6", "e4e5", "d7d5"] {
        play(&mut state, uci);
    }
    let mut expired = state.clone();
    play(&mut state, "e5d6");
    assert_eq!(piece(&state, "d6"), "P");
    assert_eq!(piece(&state, "d5"), serde_json::Value::Null);
    assert_eq!(state.halfmove_clock, 0);
    for uci in ["g1f3", "a6a5"] {
        play(&mut expired, uci);
    }
    rejects(&mut expired, 0, action("e5d6"), RuleError::IllegalMove);
    let mut pinned = ChessState::from_fen("k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    rejects(&mut pinned, 0, action("e5d6"), RuleError::IllegalMove);
}

#[test]
fn promotion_requires_choice_and_all_four_pieces_are_supported() {
    for promotion in ["q", "r", "b", "n"] {
        let mut state = ChessState::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        rejects(&mut state, 0, action("a7a8"), RuleError::IllegalMove);
        play(&mut state, &format!("a7a8{promotion}"));
        assert_eq!(piece(&state, "a8"), promotion.to_ascii_uppercase());
    }
    let mut capture = ChessState::from_fen("1r2k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    play(&mut capture, "a7b8n");
    assert_eq!(piece(&capture, "b8"), "N");
}

#[test]
fn checkmate_stalemate_resignation_and_postgame_rejections() {
    let mut mate = ChessState::initial();
    for uci in ["f2f3", "e7e5", "g2g4", "d8h4"] {
        play(&mut mate, uci);
    }
    assert_eq!(mate.winner, Some(1));
    assert!(!mate.draw);
    assert!(mate.legal_moves().is_empty());
    rejects(&mut mate, 0, action("e1f2"), RuleError::GameFinished);
    let mut stalemate = ChessState::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    assert!(stalemate.draw);
    assert_eq!(stalemate.draw_reason.as_deref(), Some("stalemate"));
    rejects(
        &mut stalemate,
        1,
        ChessAction::Resign,
        RuleError::GameFinished,
    );
    let mut resigned = ChessState::initial();
    resigned.apply(1, &ChessAction::Resign).unwrap();
    assert_eq!(resigned.winner, Some(0));
}

#[test]
fn dead_material_draws_but_two_knights_or_opposite_bishops_remain_playable() {
    for fen in [
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/2B1K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/1N2K3 w - - 0 1",
        "4kb2/8/8/8/8/8/8/2B1K3 w - - 0 1",
    ] {
        let state = ChessState::from_fen(fen).unwrap();
        assert_eq!(state.draw_reason.as_deref(), Some("insufficient_material"));
    }
    for fen in [
        "4k3/8/8/8/8/8/8/1N2K1N1 w - - 0 1",
        "2b1k3/8/8/8/8/8/8/2B1K3 w - - 0 1",
    ] {
        assert!(!ChessState::from_fen(fen).unwrap().draw);
    }
}

#[test]
fn repetition_claims_and_automatic_fivefold_survive_serialization() {
    let mut state = ChessState::initial();
    for _ in 0..2 {
        for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            play(&mut state, uci);
        }
    }
    assert!(!state.is_complete());
    assert_eq!(state.view_for(0)["can_claim_draw"], true);
    let mut claimed: ChessState =
        serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    claimed.apply(0, &ChessAction::ClaimDraw).unwrap();
    assert_eq!(claimed.draw_reason.as_deref(), Some("threefold_repetition"));
    for _ in 0..2 {
        for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            play(&mut state, uci);
        }
    }
    assert_eq!(state.draw_reason.as_deref(), Some("fivefold_repetition"));
}

#[test]
fn fifty_move_claim_seventy_five_automatic_and_mate_precedence() {
    let mut fifty = ChessState::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 99 1").unwrap();
    play(&mut fifty, "a1a2");
    assert_eq!(fifty.halfmove_clock, 100);
    assert!(!fifty.is_complete());
    assert_eq!(fifty.view_for(1)["can_claim_draw"], true);
    let mut claimed = fifty.clone();
    claimed.apply(1, &ChessAction::ClaimDraw).unwrap();
    assert_eq!(claimed.draw_reason.as_deref(), Some("fifty_move_rule"));
    play(&mut fifty, "e8f8");
    assert_eq!(fifty.halfmove_clock, 101);
    assert!(fifty.fen.contains(" 101 "));
    let mut automatic = ChessState::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 149 1").unwrap();
    play(&mut automatic, "a1a2");
    assert_eq!(
        automatic.draw_reason.as_deref(),
        Some("seventy_five_move_rule")
    );
    let mut mate = ChessState::from_fen("7k/8/5KQ1/8/8/8/8/8 w - - 149 1").unwrap();
    play(&mut mate, "g6g7");
    assert_eq!(mate.winner, Some(0));
    assert!(!mate.draw);
}

#[test]
fn pawn_moves_and_captures_reset_clock_and_repetition_history() {
    let mut pawn = ChessState::from_fen("4k3/8/8/8/8/8/P7/R3K3 w - - 99 1").unwrap();
    play(&mut pawn, "a2a3");
    assert_eq!(pawn.halfmove_clock, 0);
    assert_eq!(pawn.position_history.len(), 1);
    let mut capture = ChessState::from_fen("4k3/8/8/8/8/n7/8/R3K3 w - - 99 1").unwrap();
    play(&mut capture, "a1a3");
    assert_eq!(capture.halfmove_clock, 0);
    assert_eq!(capture.position_history.len(), 1);
}

#[test]
fn repetition_identity_includes_castling_and_only_effective_en_passant() {
    let mut rights = ChessState::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 8 5").unwrap();
    let no_rights = rights.fen.replace(" KQkq ", " Qkq ");
    rights.position_history = vec![no_rights.clone(), no_rights, rights.fen.clone()];
    assert_eq!(rights.view_for(0)["can_claim_draw"], false);

    let mut effective = ChessState::from_fen("k7/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    let no_ep = effective.fen.replace(" d6 ", " - ");
    effective.position_history = vec![no_ep.clone(), no_ep, effective.fen.clone()];
    assert_eq!(effective.view_for(0)["can_claim_draw"], false);

    let mut ineffective = ChessState::from_fen("k3r3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    let no_ep = ineffective.fen.replace(" d6 ", " - ");
    ineffective.position_history = vec![no_ep.clone(), no_ep, ineffective.fen.clone()];
    assert_eq!(ineffective.view_for(0)["can_claim_draw"], true);
}

#[test]
fn malformed_persisted_fen_is_rejected_without_panicking_or_mutation() {
    for fen in [
        "",
        "8/8/8/8/8/8/8/8 w - - 0 1",
        "8/8/8/8/8/8/8/K6k w - - nope 1",
    ] {
        assert_eq!(ChessState::from_fen(fen), Err(RuleError::InvalidState));
    }
    let mut state = ChessState::initial();
    state.fen = "corrupt".into();
    rejects(&mut state, 0, action("e2e4"), RuleError::InvalidState);
    assert_eq!(state.view_for(0)["board"].as_array().unwrap().len(), 64);
}

#[test]
fn resignation_cannot_award_a_win_to_a_bare_king() {
    let mut state = ChessState::from_fen("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
    state.apply(0, &ChessAction::Resign).unwrap();
    assert_eq!(state.winner, None);
    assert_eq!(
        state.draw_reason.as_deref(),
        Some("resignation_without_mating_material")
    );
}
