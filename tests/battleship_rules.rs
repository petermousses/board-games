use board_games::domain::battleship::{
    BattleshipAction, BattleshipState, RuleError, ShipPlacement,
};

fn fleet(column: u8, horizontal: bool) -> BattleshipAction {
    BattleshipAction::PlaceFleet {
        ships: (0..5)
            .map(|row| ShipPlacement {
                row: if horizontal { row } else { 0 },
                column: if horizontal { column } else { row },
                horizontal,
            })
            .collect(),
    }
}

fn ready() -> BattleshipState {
    let mut state = BattleshipState::initial();
    state.apply(0, &fleet(0, true)).unwrap();
    state.apply(1, &fleet(5, true)).unwrap();
    state
}

fn rejects(state: &mut BattleshipState, player: u8, action: &BattleshipAction, error: RuleError) {
    let before = state.clone();
    assert_eq!(state.apply(player, action), Err(error));
    assert_eq!(*state, before, "a rejected action must be atomic");
}

#[test]
fn setup_validates_the_entire_fleet_before_locking_it() {
    let mut state = BattleshipState::initial();
    rejects(&mut state, 2, &fleet(0, true), RuleError::InvalidPlayer);
    rejects(
        &mut state,
        0,
        &BattleshipAction::Fire { row: 0, column: 0 },
        RuleError::NotReady,
    );
    rejects(&mut state, 0, &fleet(6, true), RuleError::InvalidFleet);
    rejects(&mut state, 0, &fleet(255, true), RuleError::InvalidFleet);
    rejects(
        &mut state,
        0,
        &BattleshipAction::PlaceFleet {
            ships: vec![
                ShipPlacement {
                    row: 0,
                    column: 0,
                    horizontal: true
                };
                5
            ],
        },
        RuleError::InvalidFleet,
    );
    rejects(
        &mut state,
        0,
        &BattleshipAction::PlaceFleet { ships: vec![] },
        RuleError::InvalidFleet,
    );
    state.apply(0, &fleet(0, false)).unwrap();
    assert_eq!(state.view_for(0)["phase"], "setup");
    rejects(
        &mut state,
        0,
        &fleet(0, true),
        RuleError::FleetAlreadyPlaced,
    );
    state.apply(1, &fleet(5, true)).unwrap();
    assert_eq!(state.view_for(0)["phase"], "playing");
}

#[test]
fn views_reveal_only_own_fleet_and_previous_shots() {
    let mut state = ready();
    for player in [0, 1, 255] {
        let view = state.view_for(player);
        assert!(view.get("fleets").is_none());
        assert!(view.get("shots").is_none());
        assert!(
            view["target_board"]
                .as_array()
                .unwrap()
                .iter()
                .all(|cell| cell == 0)
        );
    }
    assert_eq!(state.view_for(0)["own_board"][0], 1);
    assert_eq!(state.view_for(1)["own_board"][0], 0);
    assert_eq!(state.view_for(255)["own_board"][0], 0);
    state
        .apply(0, &BattleshipAction::Fire { row: 0, column: 5 })
        .unwrap();
    assert_eq!(state.view_for(0)["target_board"][5], 3);
    assert_eq!(state.view_for(0)["target_board"][6], 0);
    assert_eq!(state.view_for(1)["own_board"][5], 3);
    state
        .apply(1, &BattleshipAction::Fire { row: 9, column: 9 })
        .unwrap();
    assert_eq!(state.view_for(1)["target_board"][99], 2);
    assert_eq!(state.view_for(0)["own_board"][99], 2);
}

#[test]
fn turns_targets_and_repeat_shots_are_enforced_atomically() {
    let mut state = ready();
    rejects(
        &mut state,
        1,
        &BattleshipAction::Fire { row: 0, column: 0 },
        RuleError::OutOfTurn,
    );
    rejects(
        &mut state,
        0,
        &BattleshipAction::Fire {
            row: 255,
            column: 255,
        },
        RuleError::InvalidTarget,
    );
    state
        .apply(0, &BattleshipAction::Fire { row: 9, column: 9 })
        .unwrap();
    state
        .apply(1, &BattleshipAction::Fire { row: 9, column: 9 })
        .unwrap();
    rejects(
        &mut state,
        0,
        &BattleshipAction::Fire { row: 9, column: 9 },
        RuleError::AlreadyTargeted,
    );
}

#[test]
fn sinking_the_entire_fleet_finishes_the_game() {
    let mut state = ready();
    let targets: Vec<(u8, u8)> = [5, 4, 3, 3, 2]
        .into_iter()
        .enumerate()
        .flat_map(|(row, length)| (5..5 + length).map(move |column| (row as u8, column)))
        .collect();
    for (shot, (row, column)) in targets.iter().copied().enumerate() {
        state
            .apply(0, &BattleshipAction::Fire { row, column })
            .unwrap();
        if shot + 1 < targets.len() {
            state
                .apply(
                    1,
                    &BattleshipAction::Fire {
                        row: 8 + shot as u8 / 10,
                        column: shot as u8 % 10,
                    },
                )
                .unwrap();
        }
    }
    assert!(state.is_complete());
    assert_eq!(state.view_for(0)["winner"], 0);
    for (row, column) in targets {
        assert_eq!(
            state.view_for(0)["target_board"][usize::from(row) * 10 + usize::from(column)],
            4
        );
    }
    rejects(
        &mut state,
        1,
        &BattleshipAction::Fire { row: 0, column: 0 },
        RuleError::GameFinished,
    );
    let restored: BattleshipState =
        serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
}
