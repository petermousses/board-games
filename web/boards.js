import { cardLabel, cardRed, fleetCells, playerIndex, samePile, squareName } from "./lib.js";

const C = { cream: "#d9c8a7", wood: "#527365", selected: "#e8b555", legal: "#a4c7ac", coral: "#d96950", ink: "#25332e", white: "#f6ebd3", ocean: "#345b6c" };
const blank = (width, depth) => ({ width, depth, tiles: [], pieces: [], labels: [], links: [] });
const tile = (model, x, z, color, label, pick, options = {}) => model.tiles.push({ x, z, width: 0.98, depth: 0.98, color, label, pick, ...options });
const piece = (model, x, z, color, type, pick, options = {}) => model.pieces.push({ x, z, color, type, pick, ...options });

export function boardModel(session, draft = {}, compact = false) {
  const state = session.state.state;
  const you = playerIndex(session);
  const model = blank(8.5, 8.5);
  if (session.game_type === "chess" || session.game_type === "checkers") {
    for (let index = 0; index < 64; index += 1) {
      const row = Math.floor(index / 8), column = index % 8, x = column - 3.5, z = row - 3.5;
      const dark = (row + column) % 2 === 1;
      if (session.game_type === "chess") {
        const square = squareName(index), p = state.board[index];
        const selected = draft.from === square;
        const legal = draft.from && state.legal_moves?.some((move) => move.startsWith(draft.from + square));
        const pick = { kind: "chess", square };
        tile(model, x, z, selected ? C.selected : legal ? C.legal : dark ? C.wood : C.cream, `${square}${p ? ` ${p === p.toUpperCase() ? "white" : "black"} ${chessName(p)}` : " empty"}`, pick);
        if (p) piece(model, x, z, p === p.toUpperCase() ? C.white : C.ink, "chess", pick, { role: p.toLowerCase() });
      } else {
        const square = row * 4 + Math.floor(column / 2), p = dark ? state.board[square] : 0;
        const selected = dark && (draft.from === square || draft.path?.includes(square));
        const pick = dark ? { kind: "checkers", square } : null;
        tile(model, x, z, selected ? C.selected : dark ? C.wood : C.cream, `square ${square}${p ? ` ${p < 3 ? "red" : "black"} ${p === 2 || p === 4 ? "king" : "piece"}` : " empty"}`, pick);
        if (p) piece(model, x, z, p < 3 ? C.coral : C.ink, "checker", pick, { king: p === 2 || p === 4 });
      }
    }
    for (let i = 0; i < 8; i += 1) {
      model.labels.push({ x: i - 3.5, z: 4.15, text: "abcdefgh"[i], width: 0.4, depth: 0.35 });
      model.labels.push({ x: -4.15, z: i - 3.5, text: String(8 - i), width: 0.35, depth: 0.35 });
    }
    return model;
  }
  if (["connect_four", "reversi", "tic_tac_toe"].includes(session.game_type)) {
    const columns = session.game_type === "connect_four" ? 7 : session.game_type === "reversi" ? 8 : 3;
    const rows = session.game_type === "connect_four" ? 6 : columns;
    const grid = blank(columns + 0.7, rows + 0.7);
    for (let index = 0; index < rows * columns; index += 1) {
      const p = state.board[index], column = index % columns, row = Math.floor(index / columns);
      const x = column - (columns - 1) / 2, z = row - (rows - 1) / 2;
      const pick = { kind: session.game_type, square: index, column };
      const legal = state.turn === you && state.legal_moves?.includes(session.game_type === "connect_four" ? column : index);
      const color = session.game_type === "reversi" ? (legal ? "#799d83" : C.wood) : (legal ? "#b0c1aa" : C.cream);
      tile(grid, x, z, color, `row ${row + 1}, column ${column + 1}: ${p ? `player ${p}` : "empty"}`, pick);
      if (p) piece(grid, x, z, session.game_type === "reversi" ? (p === 1 ? C.ink : C.white) : (p === 1 ? C.coral : C.white), session.game_type === "tic_tac_toe" ? (p === 1 ? "cross" : "ring") : "checker", pick);
    }
    return grid;
  }
  if (session.game_type === "solitaire") return solitaireModel(state, draft);
  if (session.game_type === "battleship") return battleshipModel(state, draft, compact);
  if (session.game_type === "clue") return clueModel(state);
  return model;
}

function solitaireModel(state, draft) {
  const maxCards = Math.max(9, ...state.tableau.map((pile) => pile.cards.length));
  const depth = 4.5 + maxCards * 0.3;
  const model = blank(9.4, depth);
  const top = -depth / 2 + 1.2;
  const x = (column) => (column - 3) * 1.25;
  const addCard = (card, xx, z, label, pick, selected = false, y = 0.11) => piece(model, xx, z, cardRed(card) ? "#a83439" : "#263932", "card", pick, { text: cardLabel(card), width: 1.04, depth: 1.45, label, selected, y });
  tile(model, x(0), top, "#264b48", state.stock.length ? `draw from stock: ${state.stock.length} cards` : "recycle waste", { kind: "draw" }, { width: 1.04, depth: 1.45 });
  piece(model, x(0), top, "#315b69", "card", { kind: "draw" }, { text: state.stock.length ? "▧" : "↻", back: true, width: 1.04, depth: 1.45 });
  model.labels.push({ x: x(0), z: top - 1, text: `stock · ${state.stock.length}`, width: 1.2, depth: 0.3 });
  const pile = (cards, source, column, label) => {
    const pick = { kind: "solitaire", from: cards.length ? source : null, to: source, count: 1 };
    tile(model, x(column), top, "#365d4e", `${label}: ${cards.length ? cardLabel(cards.at(-1)) : "empty"}`, pick, { width: 1.04, depth: 1.45 });
    if (cards.length) addCard(cards.at(-1), x(column), top, label, pick, samePile(draft.from, source));
    model.labels.push({ x: x(column), z: top - 1, text: label, width: 1.2, depth: 0.3 });
  };
  pile(state.waste, { kind: "waste" }, 1, "waste");
  state.foundations.forEach((cards, index) => pile(cards, { kind: "foundation", index }, index + 3, ["♠", "♥", "♦", "♣"][index]));
  state.tableau.forEach((pile, index) => {
    const source = { kind: "tableau", index };
    const start = top + 2;
    tile(model, x(index), start, "#365d4e", `column ${index + 1} empty space`, { kind: "solitaire", from: null, to: source, count: 0 }, { width: 1.04, depth: 1.45 });
    pile.cards.forEach((card, cardIndex) => {
      const faceUp = cardIndex >= pile.face_up_from;
      const count = pile.cards.length - cardIndex;
      const pick = faceUp ? { kind: "solitaire", from: source, to: source, count } : null;
      const selected = samePile(draft.from, source) && count <= draft.count;
      if (faceUp) addCard(card, x(index), start + cardIndex * 0.3, `${cardLabel(card)}, column ${index + 1}, ${count} card${count === 1 ? "" : "s"}`, pick, selected, 0.13 + cardIndex * 0.025);
      else piece(model, x(index), start + cardIndex * 0.3, "#315b69", "card", null, { back: true, text: "", width: 1.04, depth: 1.45, y: 0.13 + cardIndex * 0.025 });
    });
  });
  return model;
}

function battleshipModel(state, draft, compact) {
  const model = blank(compact ? 7.6 : 15.3, compact ? 16.1 : 8.1);
  const preview = fleetCells(draft.ships || []) || new Set();
  for (let side = 0; side < 2; side += 1) {
    const cx = compact ? 0 : (side === 0 ? -3.85 : 3.85);
    const cz = compact ? (side === 0 ? -4 : 4) : 0;
    model.labels.push({ x: cx, z: cz - 3.8, text: side === 0 ? "your fleet" : "opponent waters", width: 5, depth: 0.5 });
    for (let index = 0; index < 100; index += 1) {
      const row = Math.floor(index / 10), column = index % 10;
      const x = cx + (column - 4.5) * 0.66, z = cz + (row - 4.5) * 0.66;
      const p = (side === 0 ? state.own_board : state.target_board)?.[index] || 0;
      const ship = p === 1 || (side === 0 && state.phase === "setup" && preview.has(index));
      const pick = { kind: "battleship", own: side === 0, row, column };
      const status = p === 4 ? "sunk" : p === 3 ? "hit" : p === 2 ? "miss" : ship ? "ship" : "untried";
      tile(model, x, z, p === 4 ? "#9b7541" : ship ? "#acbcb5" : C.ocean, `${side === 0 ? "own" : "target"} ${String.fromCharCode(65 + column)}${row + 1}: ${status}`, pick, { width: 0.63, depth: 0.63 });
      if (ship) piece(model, x, z, "#d0d5c4", "ship", pick, { scale: 0.55 });
      if (p >= 2) piece(model, x, z, p === 4 ? "#e6b957" : p === 3 ? C.coral : C.white, p === 4 ? "sunk" : "pin", pick, { scale: 0.6 });
    }
  }
  return model;
}

function clueModel(state) {
  const nodes = state.nodes || [];
  if (!nodes.length) return blank(10, 10);
  const minX = Math.min(...nodes.map((n) => n.x)), maxX = Math.max(...nodes.map((n) => n.x));
  const minZ = Math.min(...nodes.map((n) => n.z)), maxZ = Math.max(...nodes.map((n) => n.z));
  const scale = 10 / Math.max(maxX - minX, maxZ - minZ, 1);
  const pos = (node) => ({ x: (node.x - (minX + maxX) / 2) * scale, z: (node.z - (minZ + maxZ) / 2) * scale });
  const model = blank(13.5, 13.5);
  for (const node of nodes) {
    const { x, z } = pos(node);
    for (const neighbor of node.neighbors || []) {
      const other = nodes.find((item) => item.id === neighbor);
      if (other && neighbor > node.id) model.links.push({ from: { x, z }, to: pos(other) });
    }
    const room = node.kind === "room";
    const legal = state.legal_destinations?.includes(node.id);
    const pick = { kind: "clue", destination: node.id };
    tile(model, x, z, legal ? "#a4c7ac" : room ? C.cream : C.wood, `${node.name}${legal ? ", reachable" : ""}`, pick, { width: room ? 2.9 : 0.75, depth: room ? 2.7 : 0.75 });
    if (room) model.labels.push({ x, z: z - 0.8, text: node.name, width: 2.5, depth: 0.5 });
  }
  const colors = [C.coral, "#d6ac46", "#f2e9d7", "#5e8a69", "#4c819a", "#8c648e"];
  (state.suspect_positions || state.positions || []).forEach((position, index) => {
    const node = nodes.find((n) => n.id === position);
    if (!node) return;
    const { x, z } = pos(node);
    piece(model, x + (index % 3 - 1) * 0.65, z + Math.floor(index / 3) * 0.7, colors[index], "pawn", { kind: "clue", destination: node.id }, { scale: 0.7 });
  });
  return model;
}

function chessName(char) { return ({ p: "pawn", n: "knight", b: "bishop", r: "rook", q: "queen", k: "king" })[char.toLowerCase()]; }
