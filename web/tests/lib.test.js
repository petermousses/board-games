import test from "node:test";
import assert from "node:assert/strict";
import { GAMES, cardLabel, fleetCells, randomFleet, ResponseGate, parseSaved, mergeSavedSessions, squareName, trimmedFormValue } from "../lib.js";
import { boardModel } from "../boards.js";

test("card zero is the ace of spades; only null/undefined are absent", () => {
  assert.equal(cardLabel(0), "A♠");
  assert.equal(cardLabel(null), "");
  assert.equal(cardLabel(51), "K♣");
});
test("late requests cannot replace another route or rewind the board", () => {
  const gate = new ResponseGate();
  const old = gate.enter("a");
  assert.ok(gate.accepts(old, { id: "a", state_version: 3 }));
  assert.equal(gate.accepts(old, { id: "a", state_version: 2 }), false);
  const current = gate.enter("b");
  assert.equal(gate.accepts(old, { id: "a", state_version: 4 }), false);
  assert.ok(gate.accepts(current, { id: "b", state_version: 0 }));
  gate.enter("a");
  assert.equal(gate.accepts(old, { id: "a", state_version: 10 }), false);
});
test("fleet preview rejects overlap and edges, and always offers a complete layout", () => {
  assert.equal(fleetCells([{ row: 0, column: 6, horizontal: true }]), null);
  assert.equal(fleetCells([{ row: 0, column: 0, horizontal: true }, { row: 0, column: 0, horizontal: false }]), null);
  for (const random of [() => 0, () => 0.999, Math.random]) {
    const ships = randomFleet(random);
    assert.equal(ships.length, 5);
    assert.equal(fleetCells(ships).size, 17);
  }
});
test("corrupt or injected local session records are discarded", () => {
  for (const value of ["null", "[]", "true", "nope", '{"bad":{"id":"bad"}}']) assert.deepEqual(parseSaved(value), {});
  const id = "00000000-0000-0000-0000-000000000001";
  const session = { id, game_type: "chess", access_token: "a".repeat(43) };
  assert.deepEqual(parseSaved(JSON.stringify({ [id]: session })), { [id]: session });
});
test("v1 saved games migrate into the current session store", () => {
  const id = "00000000-0000-0000-0000-000000000001";
  const legacy = { [id]: { id, game_type: "checkers", access_token: "a".repeat(43), label: "old seat" } };
  const current = { [id]: { id, game_type: "chess", access_token: "b".repeat(43), label: "new seat" } };
  assert.deepEqual(mergeSavedSessions(null, JSON.stringify(legacy)), legacy);
  assert.deepEqual(mergeSavedSessions(JSON.stringify(current), JSON.stringify(legacy)), current);
});
test("captured form values survive disabling the submitted controls", () => {
  const values = new FormData();
  values.append("display_name", "  player two  ");
  assert.equal(trimmedFormValue(values, "display_name"), "player two");
  assert.equal(trimmedFormValue(new FormData(), "display_name"), "");
});
test("chess display maps row-major a8 through h1", () => {
  assert.equal(squareName(0), "a8");
  assert.equal(squareName(63), "h1");
});
test("every advertised game creates a Three.js board model", () => {
  const session = (game_type, state) => ({ game_type, you: { player_index: 0 }, state: { state } });
  const samples = {
    chess: session("chess", { board: Array(64).fill(null), legal_moves: [], turn: 0 }),
    checkers: session("checkers", { board: Array(32).fill(0), side_to_move: "red" }),
    solitaire: session("solitaire", { stock: [], waste: [], foundations: [[], [], [], []], tableau: Array.from({ length: 7 }, () => ({ cards: [], face_up_from: 0 })) }),
    battleship: session("battleship", { phase: "setup", own_board: Array(100).fill(0), target_board: Array(100).fill(0), turn: 0 }),
    clue: session("clue", { nodes: [{ id: 0, name: "atrium", kind: "room", x: 0, z: 0, neighbors: [] }], legal_destinations: [0], suspect_positions: [0] }),
    connect_four: session("connect_four", { board: Array(42).fill(0), legal_moves: [0], turn: 0 }),
    reversi: session("reversi", { board: Array(64).fill(0), legal_moves: [], turn: 0 }),
    tic_tac_toe: session("tic_tac_toe", { board: Array(9).fill(0), legal_moves: [0], turn: 0 }),
  };
  assert.deepEqual(Object.keys(samples).sort(), Object.keys(GAMES).sort());
  for (const [game, gameSession] of Object.entries(samples)) {
    const model = boardModel(gameSession);
    assert.ok(model.width > 0 && model.depth > 0, `${game} sets table dimensions`);
    assert.ok(model.tiles.length > 0, `${game} renders playable spaces`);
  }
});
test("battleship shows a sunk ship differently from an ordinary hit", () => {
  const state = { phase: "playing", own_board: Array(100).fill(0), target_board: Array(100).fill(0), turn: 0 };
  const session = (target) => ({ game_type: "battleship", you: { player_index: 0 }, state: { state: { ...state, target_board: target } } });
  const hit = [...state.target_board];
  const sunk = [...state.target_board];
  hit[5] = 3;
  sunk[5] = 4;
  const targetAt = (model) => model.tiles.find((item) => !item.pick.own && item.pick.row === 0 && item.pick.column === 5);
  const markerAt = (model) => model.pieces.find((item) => !item.pick.own && item.pick.row === 0 && item.pick.column === 5);
  assert.equal(targetAt(boardModel(session(hit))).label, "target F1: hit");
  assert.equal(targetAt(boardModel(session(sunk))).label, "target F1: sunk");
  assert.equal(markerAt(boardModel(session(hit))).type, "pin");
  assert.equal(markerAt(boardModel(session(sunk))).type, "sunk");
});
