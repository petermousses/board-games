use std::collections::BTreeSet;

use board_games::domain::clue::{ClueAction, ClueState, RuleError};
use serde_json::{Value, json};

fn started(players: u8) -> ClueState {
    let mut state = ClueState::seeded(0x0BAD_5EED);
    state.start(players).unwrap();
    state
}

fn rejects(state: &mut ClueState, player: u8, action: ClueAction, error: RuleError) {
    let before = state.clone();
    assert_eq!(state.apply(player, &action), Err(error));
    assert_eq!(
        *state, before,
        "a rejected action must preserve the entire state"
    );
}

fn raw(state: &ClueState) -> Value {
    serde_json::to_value(state).unwrap()
}

fn fixture(mut state: ClueState, edit: impl FnOnce(&mut Value)) -> ClueState {
    let mut value = raw(&state);
    edit(&mut value);
    state = serde_json::from_value(value).unwrap();
    state
}

fn assert_private_keys_absent(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                assert!(
                    !["seed", "solution", "hands"].contains(&key.as_str()),
                    "secret key {key} leaked"
                );
                assert_private_keys_absent(value);
            }
        }
        Value::Array(array) => array.iter().for_each(assert_private_keys_absent),
        _ => {}
    }
}

#[test]
fn lobby_has_no_hands_and_start_enforces_three_to_six_players() {
    let mut state = ClueState::seeded(44);
    let before = state.clone();
    for players in [0, 1, 2, 7, 255] {
        assert_eq!(state.start(players), Err(RuleError::InvalidPlayerCount));
        assert_eq!(state, before);
    }
    assert_eq!(state.view_for(0)["phase"], "lobby");
    assert_eq!(state.view_for(0)["your_hand"], json!([]));
    assert_eq!(state.view_for(0)["positions"], json!([]));
    assert_private_keys_absent(&state.view_for(0));
    rejects(&mut state, 0, ClueAction::EndTurn, RuleError::NotStarted);
    state.start(3).unwrap();
    let before = state.clone();
    assert_eq!(state.start(4), Err(RuleError::AlreadyStarted));
    assert_eq!(state, before);
}

#[test]
fn all_deals_have_one_solution_per_category_and_every_other_card_exactly_once() {
    for players in 3..=6 {
        for seed in 0..24 {
            let mut state = ClueState::seeded(seed);
            state.start(players).unwrap();
            let storage = raw(&state);
            assert_eq!(storage["seed"], Value::Null);
            let solution = storage["solution"].as_array().unwrap();
            assert!(solution[0].as_u64().unwrap() < 6);
            assert!((6..12).contains(&solution[1].as_u64().unwrap()));
            assert!((12..21).contains(&solution[2].as_u64().unwrap()));
            let mut all_cards: Vec<u64> =
                solution.iter().map(|card| card.as_u64().unwrap()).collect();
            let mut sizes = Vec::new();
            for player in 0..players {
                let view = state.view_for(player);
                let hand = view["your_hand"].as_array().unwrap();
                assert_eq!(&storage["hands"][usize::from(player)], &view["your_hand"]);
                sizes.push(hand.len());
                all_cards.extend(hand.iter().map(|card| card.as_u64().unwrap()));
                assert_private_keys_absent(&view);
            }
            assert_eq!(all_cards.len(), 21);
            assert_eq!(
                all_cards.into_iter().collect::<BTreeSet<_>>(),
                (0..21).collect()
            );
            assert!(sizes.iter().max().unwrap() - sizes.iter().min().unwrap() <= 1);
        }
    }
}

#[test]
fn player_views_do_not_depend_on_unrevealed_opponents_hands_or_solution() {
    let state = started(3);
    let changed = fixture(state.clone(), |value| {
        let hand1 = value["hands"][1].clone();
        value["hands"][1] = value["hands"][2].clone();
        value["hands"][2] = hand1;
        value["solution"] = json!([5, 11, 20]);
    });
    assert_eq!(state.view_for(0), changed.view_for(0));
    let stranger = state.view_for(255);
    assert_eq!(stranger["your_hand"], json!([]));
    assert_eq!(stranger["refutable_cards"], json!([]));
    assert_eq!(stranger["shown_card"], Value::Null);
    assert!(!stranger["can_suggest"].as_bool().unwrap());
    assert_private_keys_absent(&stranger);
}

#[test]
fn movement_uses_the_public_graph_and_rejects_invalid_or_repeated_moves() {
    let mut state = started(3);
    let view = state.view_for(0);
    assert_eq!(view["positions"][0], 0);
    assert_eq!(view["nodes"].as_array().unwrap().len(), 21);
    assert_eq!(view["legal_destinations"], json!([1, 3, 9, 15]));
    rejects(
        &mut state,
        1,
        ClueAction::Move { destination: 1 },
        RuleError::OutOfTurn,
    );
    rejects(&mut state, 3, ClueAction::EndTurn, RuleError::InvalidPlayer);
    for destination in [0, 8, 21, 255] {
        rejects(
            &mut state,
            0,
            ClueAction::Move { destination },
            RuleError::InvalidMove,
        );
    }
    state
        .apply(0, &ClueAction::Move { destination: 9 })
        .unwrap();
    assert_eq!(state.view_for(0)["positions"][0], 9);
    rejects(
        &mut state,
        0,
        ClueAction::Move { destination: 1 },
        RuleError::InvalidMove,
    );
    rejects(
        &mut state,
        0,
        ClueAction::Suggest {
            suspect: 0,
            weapon: 0,
        },
        RuleError::InvalidSuggestion,
    );
    state.apply(0, &ClueAction::EndTurn).unwrap();
    state.apply(1, &ClueAction::EndTurn).unwrap();
    state.apply(2, &ClueAction::EndTurn).unwrap();
    state
        .apply(0, &ClueAction::Move { destination: 1 })
        .unwrap();
    assert!(state.view_for(0)["can_suggest"].as_bool().unwrap());
}

fn refutation_fixture() -> ClueState {
    // Known, complete partition of 21 cards: two later players can refute the
    // suggestion [suspect 0, weapon 0, Kitchen]. Clockwise seat 1 must answer.
    fixture(started(3), |value| {
        value["solution"] = json!([5, 11, 20]);
        value["hands"] = json!([
            [1, 2, 3, 4, 7, 8],
            [0, 6, 9, 10, 13, 14],
            [12, 15, 16, 17, 18, 19]
        ]);
    })
}

#[test]
fn clockwise_first_refuter_chooses_a_matching_private_card_and_blocks_other_actions() {
    let mut state = refutation_fixture();
    state
        .apply(
            0,
            &ClueAction::Suggest {
                suspect: 0,
                weapon: 0,
            },
        )
        .unwrap();
    assert_eq!(state.view_for(0)["pending_refuter"], 1);
    assert_eq!(state.view_for(1)["refutable_cards"], json!([0, 6]));
    assert_eq!(state.view_for(0)["refutable_cards"], json!([]));
    assert_eq!(state.view_for(2)["refutable_cards"], json!([]));
    rejects(
        &mut state,
        2,
        ClueAction::Refute { card: 12 },
        RuleError::InvalidRefuter,
    );
    rejects(
        &mut state,
        1,
        ClueAction::Refute { card: 9 },
        RuleError::InvalidRefutation,
    );
    rejects(
        &mut state,
        1,
        ClueAction::Refute { card: 12 },
        RuleError::InvalidRefutation,
    );
    rejects(
        &mut state,
        0,
        ClueAction::EndTurn,
        RuleError::RefutationPending,
    );
    rejects(
        &mut state,
        0,
        ClueAction::Accuse {
            suspect: 5,
            weapon: 5,
            room: 8,
        },
        RuleError::RefutationPending,
    );
    state.apply(1, &ClueAction::Refute { card: 6 }).unwrap();
    assert_eq!(state.view_for(0)["shown_card"], 6);
    for player in [1, 2, 255] {
        assert_eq!(state.view_for(player)["shown_card"], Value::Null);
        assert_eq!(state.view_for(player)["suggestion"].get("shown_card"), None);
        assert_private_keys_absent(&state.view_for(player));
    }
    assert_eq!(state.view_for(0)["suggestion"]["status"], "refuted");
    rejects(
        &mut state,
        1,
        ClueAction::Refute { card: 0 },
        RuleError::InvalidRefuter,
    );
    rejects(
        &mut state,
        0,
        ClueAction::Suggest {
            suspect: 0,
            weapon: 0,
        },
        RuleError::InvalidSuggestion,
    );
    rejects(
        &mut state,
        0,
        ClueAction::Move { destination: 1 },
        RuleError::InvalidMove,
    );
    state.apply(0, &ClueAction::EndTurn).unwrap();
    assert_eq!(state.view_for(1)["turn"], 1);
}

#[test]
fn unrefuted_suggestions_move_suspects_and_allow_an_accusation_to_win() {
    let mut state = fixture(refutation_fixture(), |value| {
        value["positions"][0] = json!(8);
    });
    state
        .apply(
            0,
            &ClueAction::Suggest {
                suspect: 5,
                weapon: 5,
            },
        )
        .unwrap();
    let view = state.view_for(0);
    assert_eq!(view["suggestion"]["status"], "unrefuted");
    assert_eq!(view["pending_refuter"], Value::Null);
    assert_eq!(view["suspect_positions"][5], 8);
    state
        .apply(
            0,
            &ClueAction::Accuse {
                suspect: 5,
                weapon: 5,
                room: 8,
            },
        )
        .unwrap();
    assert!(state.is_complete());
    assert_eq!(state.view_for(0)["winner"], 0);
    assert_eq!(state.view_for(0)["last_accusation"]["correct"], true);
    assert_private_keys_absent(&state.view_for(0));
    rejects(&mut state, 0, ClueAction::EndTurn, RuleError::GameFinished);
    assert_eq!(
        serde_json::from_value::<ClueState>(raw(&state)).unwrap(),
        state
    );
}

#[test]
fn eliminated_players_still_refute_but_lose_their_turn_and_accusation_right() {
    let mut state = refutation_fixture();
    state.apply(0, &ClueAction::EndTurn).unwrap();
    state
        .apply(
            1,
            &ClueAction::Accuse {
                suspect: 0,
                weapon: 0,
                room: 0,
            },
        )
        .unwrap();
    assert_eq!(state.view_for(0)["turn"], 2);
    assert_eq!(state.view_for(1)["eliminated"], json!([false, true, false]));
    rejects(
        &mut state,
        1,
        ClueAction::Accuse {
            suspect: 5,
            weapon: 5,
            room: 8,
        },
        RuleError::Eliminated,
    );
    state.apply(2, &ClueAction::EndTurn).unwrap();
    state
        .apply(
            0,
            &ClueAction::Suggest {
                suspect: 0,
                weapon: 0,
            },
        )
        .unwrap();
    assert_eq!(state.view_for(1)["pending_refuter"], 1);
    state.apply(1, &ClueAction::Refute { card: 0 }).unwrap();
    state.apply(0, &ClueAction::EndTurn).unwrap();
    assert_eq!(state.view_for(0)["turn"], 2);
}

#[test]
fn all_incorrect_accusations_end_without_a_winner_and_invalid_cards_are_atomic() {
    let mut state = refutation_fixture();
    rejects(
        &mut state,
        0,
        ClueAction::Suggest {
            suspect: 255,
            weapon: 0,
        },
        RuleError::InvalidCard,
    );
    rejects(
        &mut state,
        0,
        ClueAction::Accuse {
            suspect: 0,
            weapon: 255,
            room: 0,
        },
        RuleError::InvalidCard,
    );
    rejects(
        &mut state,
        0,
        ClueAction::Accuse {
            suspect: 0,
            weapon: 0,
            room: 9,
        },
        RuleError::InvalidCard,
    );
    for player in 0..3 {
        state
            .apply(
                player,
                &ClueAction::Accuse {
                    suspect: 0,
                    weapon: 0,
                    room: 0,
                },
            )
            .unwrap();
        assert_eq!(state.is_complete(), player == 2);
    }
    assert_eq!(state.view_for(0)["winner"], Value::Null);
    assert_eq!(state.view_for(0)["phase"], "complete");
}

#[test]
fn six_player_clockwise_order_wraps_and_summoned_player_can_suggest_next_turn() {
    let mut state = fixture(started(6), |value| {
        value["solution"] = json!([5, 11, 20]);
        value["hands"] = json!([
            [0, 6, 12],
            [1, 7, 13],
            [2, 8, 14],
            [3, 9, 15],
            [4, 10, 16],
            [17, 18, 19]
        ]);
        value["turn"] = json!(5);
        value["positions"][5] = json!(0);
    });
    state
        .apply(
            5,
            &ClueAction::Suggest {
                suspect: 1,
                weapon: 0,
            },
        )
        .unwrap();
    assert_eq!(state.view_for(5)["pending_refuter"], 0);
    assert_eq!(state.view_for(1)["positions"][1], 0);
    state.apply(0, &ClueAction::Refute { card: 6 }).unwrap();
    state.apply(5, &ClueAction::EndTurn).unwrap();
    state.apply(0, &ClueAction::EndTurn).unwrap();
    assert!(state.view_for(1)["can_suggest"].as_bool().unwrap());
}
