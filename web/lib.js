export const GAMES = {
  chess: { name: "chess", players: "2 players", min: 2, max: 2, symbol: "♞", tag: "strategy", description: "Every move opens a possibility. Make yours count.", rules: "White moves first. Select a piece, then a highlighted destination. Castling, en passant, promotion, checkmate, and draws are enforced by the server." },
  battleship: { name: "battleship", players: "2 players", min: 2, max: 2, symbol: "⚓", tag: "hidden fleets", description: "Hide your fleet. Read the ocean. Find theirs.", rules: "Place ships of lengths 5, 4, 3, 3, and 2 without overlap. Both fleets must be ready before firing. Take one shot per turn; sink every enemy ship to win." },
  clue: { name: "Clue", players: "3–6 players", min: 3, max: 6, symbol: "?", tag: "deduction", description: "A house full of secrets. A table full of suspects.", rules: "Quick-table movement: move up to two connected spaces once per turn; suggest a suspect and weapon in your room. The first player clockwise who can disprove must privately show a card. Make one final accusation: get it wrong and you can only refute. This original room map uses house movement rules, without dice." },
  checkers: { name: "checkers", players: "2 players", min: 2, max: 2, symbol: "◉", tag: "strategy", description: "A familiar board. A surprisingly sharp battle.", rules: "Red moves first. Captures are mandatory. Select a piece and every landing square in a multi-jump, then submit the complete path. Reaching the king row ends that turn." },
  solitaire: { name: "solitaire", players: "solo", min: 1, max: 1, symbol: "♠", tag: "a quiet moment", description: "A little order in the shuffle. Just you and the cards.", rules: "Draw one Klondike. Build descending alternating colors in the tableau; only kings fill empty columns. Build each foundation from ace to king in suit. Select a face-up card or sequence, then its destination." },
  connect_four: { name: "connect four", players: "2 players", min: 2, max: 2, symbol: "⁙", tag: "four in a row", description: "Drop a disc. Set a trap. Connect the dots.", rules: "Choose a column to drop your disc. Connect four horizontally, vertically, or diagonally. The first player uses coral; the second uses ivory." },
  reversi: { name: "reversi", players: "2 players", min: 2, max: 2, symbol: "◐", tag: "turn the tables", description: "One small move can change the whole board.", rules: "Black moves first. Bracket opposing discs along any straight line to flip them. Pass only when no legal move exists. When neither side can move, the most discs wins." },
  tic_tac_toe: { name: "tic-tac-toe", players: "2 players", min: 2, max: 2, symbol: "×", tag: "a quick classic", description: "Three in a row. You know what to do.", rules: "X moves first. Place on an empty square. Connect three horizontally, vertically, or diagonally; a full board without a line is a draw." },
};

export const escapeHtml = (value) => String(value ?? "").replace(/[&<>'"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;" })[c]);
export const cardLabel = (card) => card == null ? "" : `${["A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K"][card % 13]}${["♠", "♥", "♦", "♣"][Math.floor(card / 13)]}`;
export const cardRed = (card) => [1, 2].includes(Math.floor(card / 13));
export const squareName = (index) => `${"abcdefgh"[index % 8]}${8 - Math.floor(index / 8)}`;
export const playerIndex = (session) => session.you.player_index ?? ({ solitaire: 0, red: 0, black: 1 }[session.you.seat] ?? 0);
export const samePile = (a, b) => !!a && !!b && a.kind === b.kind && a.index === b.index;

export function parseSaved(raw) {
  try {
    const parsed = JSON.parse(raw);
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") return {};
    return Object.fromEntries(Object.entries(parsed).filter(([id, value]) =>
      /^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/i.test(id) && value?.id === id &&
      /^[A-Za-z0-9_-]{43}$/.test(value.access_token) && Object.hasOwn(GAMES, value.game_type),
    ));
  } catch { return {}; }
}

export function mergeSavedSessions(currentRaw, legacyRaw) {
  return { ...parseSaved(legacyRaw), ...parseSaved(currentRaw) };
}

// A response belongs to a route generation as well as a session/version. Leaving
// and reopening the same session must invalidate requests from its previous visit.
export class ResponseGate {
  generation = 0;
  id = null;
  version = -1;
  enter(id) { this.generation += 1; this.id = id; this.version = -1; return this.generation; }
  accepts(generation, session) {
    if (generation !== this.generation || session.id !== this.id || session.state_version < this.version) return false;
    this.version = session.state_version;
    return true;
  }
}

export function fleetCells(ships, lengths = [5, 4, 3, 3, 2]) {
  const cells = new Set();
  if (ships.length > lengths.length) return null;
  for (let index = 0; index < ships.length; index += 1) {
    const { row, column, horizontal } = ships[index];
    if (![row, column].every(Number.isInteger) || typeof horizontal !== "boolean") return null;
    for (let offset = 0; offset < lengths[index]; offset += 1) {
      const r = row + (horizontal ? 0 : offset);
      const c = column + (horizontal ? offset : 0);
      const cell = r * 10 + c;
      if (r < 0 || r >= 10 || c < 0 || c >= 10 || cells.has(cell)) return null;
      cells.add(cell);
    }
  }
  return cells;
}

export function randomFleet(random = Math.random) {
  const ships = [];
  for (let index = 0; index < 5; index += 1) {
    let placed = false;
    for (let attempt = 0; attempt < 1000; attempt += 1) {
      const ship = { row: Math.floor(random() * 10), column: Math.floor(random() * 10), horizontal: random() < 0.5 };
      if (fleetCells([...ships, ship])) { ships.push(ship); placed = true; break; }
    }
    if (!placed) return [0, 2, 4, 6, 8].map((row) => ({ row, column: 0, horizontal: true }));
  }
  return ships;
}
