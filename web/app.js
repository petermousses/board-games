import { GAMES, escapeHtml as escape, fleetCells, mergeSavedSessions, playerIndex, randomFleet, samePile, ResponseGate } from "./lib.js";
import { boardModel } from "./boards.js";

const API_ROOT = "/api/v1";
const SAVED_SESSIONS_KEY = "tabletop.saved-sessions.v2";
const LEGACY_SAVED_SESSIONS_KEY = "tabletop.saved-sessions.v1";
const app = document.querySelector("#app");
const announcer = document.querySelector("#announcer");
const responses = new ResponseGate();
const memorySessions = {};

let currentSession = null;
let table = null;
let Table = null;
let tableLoad = null;
let graphicsUnavailable = false;
let tableView = { top: false, orbit: false };
let draft = {};
let mutationInFlight = false;
let pollTimer = null;
let routeController = null;
let keyboardChoices = [];
let compactBoard = window.innerWidth < 800;

function savedSessions() {
  try {
    return { ...mergeSavedSessions(localStorage.getItem(SAVED_SESSIONS_KEY), localStorage.getItem(LEGACY_SAVED_SESSIONS_KEY)), ...memorySessions };
  } catch {
    return { ...memorySessions };
  }
}

function persistSessions(sessions) {
  localStorage.setItem(SAVED_SESSIONS_KEY, JSON.stringify(sessions));
  localStorage.removeItem(LEGACY_SAVED_SESSIONS_KEY);
}

function saveSession(access) {
  const record = {
    id: access.id,
    access_token: access.access_token,
    game_type: access.game_type,
    label: access.you.display_name,
    saved_at: new Date().toISOString(),
  };
  memorySessions[record.id] = record;
  try {
    persistSessions({ ...savedSessions(), [record.id]: record });
  } catch {
    toast("browser storage is unavailable. copy your private resume code before closing this tab.", true, 15000);
  }
}

function forgetSession(id) {
  const sessions = savedSessions();
  delete sessions[id];
  delete memorySessions[id];
  try {
    persistSessions(sessions);
  } catch {
    toast("your browser blocked the saved-session update.", true);
  }
  navigate();
}

async function request(path, { method = "GET", body, token, signal } = {}) {
  const headers = token ? { Authorization: `Bearer ${token}` } : {};
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(`${API_ROOT}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal,
    cache: "no-store",
  });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok) {
    const error = new Error(payload.error || `request failed (${response.status})`);
    error.status = response.status;
    throw error;
  }
  return payload;
}

function navigate(id = null) {
  history.pushState({}, "", id ? `/?session=${encodeURIComponent(id)}` : "/");
  renderRoute();
}

function stopPolling() {
  clearTimeout(pollTimer);
  pollTimer = null;
}

function disposeTable() {
  table?.dispose();
  table = null;
}

function loadTable() {
  if (!tableLoad) tableLoad = import("./table.js").then((module) => {
    Table = module.Table;
    return Table;
  });
  return tableLoad;
}

function ensureTable(sessionId) {
  if (table || graphicsUnavailable) return;
  const canvas = document.querySelector("#game-canvas");
  if (!canvas) return;
  if (!Table) {
    loadTable().then(() => {
      if (!table && currentSession?.id === sessionId && document.querySelector("#session-root")?.dataset.id === sessionId) renderSession();
    }).catch(() => {
      graphicsUnavailable = true;
      graphicsError("this browser couldn’t load the 3D table. reload to try again; your game is still saved.");
    });
    return;
  }
  try {
    table = new Table(canvas, handlePick, graphicsError);
    table.setTop(tableView.top);
    table.setOrbit(tableView.orbit);
  } catch {
    graphicsUnavailable = true;
    graphicsError("this browser couldn’t start WebGL. enable hardware acceleration or use another browser for the 3D table.");
  }
}

async function renderRoute() {
  stopPolling();
  routeController?.abort();
  routeController = new AbortController();
  disposeTable();
  currentSession = null;
  draft = {};
  mutationInFlight = false;
  graphicsUnavailable = false;
  tableView = { top: false, orbit: false };

  const id = new URLSearchParams(location.search).get("session");
  const generation = responses.enter(id);
  if (!id) return renderHome();
  if (!/^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/i.test(id)) {
    app.innerHTML = `<section class="panel message"><p class="eyebrow">invitation unavailable</p><h1>that link is incomplete.</h1><p>ask the host for the full invitation link.</p><a class="button" href="/">all games</a></section>`;
    return;
  }

  const saved = savedSessions()[id];
  if (!saved) return renderJoin(id);
  app.innerHTML = `<section class="loading"><span class="loading-disc"></span><p>setting your table…</p></section>`;
  try {
    const session = await request(`/sessions/${id}`, { token: saved.access_token, signal: routeController.signal });
    if (responses.accepts(generation, session)) {
      receive(session);
      schedulePoll();
    }
  } catch (error) {
    if (error.name === "AbortError" || generation !== responses.generation) return;
    app.innerHTML = `<section class="panel message"><p class="eyebrow">session unavailable</p><h1>couldn’t open this table.</h1><p>${escape(error.message)}</p><button id="retry-open" class="primary">try again</button><a class="button secondary" href="/">all games</a></section>`;
    document.querySelector("#retry-open").onclick = renderRoute;
  }
}

function renderHome() {
  const saved = Object.values(savedSessions()).sort((a, b) => String(b.saved_at || "").localeCompare(String(a.saved_at || "")));
  app.innerHTML = `
    <section class="hero">
      <div>
        <p class="eyebrow"><span class="live-dot"></span> make time for a good game</p>
        <h1>one table.<br><em>eight classics.</em></h1>
        <p class="lede">pull up a chair, invite your people, and let the evening unfold.</p>
        <a class="button primary" href="#start-panel">find your game <span>↘</span></a>
      </div>
      <div class="hero-art" aria-hidden="true"><div class="art-board"><span class="art-piece black">♞</span><span class="art-piece ivory">♟</span><span class="art-disc"></span></div><div class="art-card">A<span>♠</span></div><div class="art-caption">the best part is who’s across the table.</div></div>
    </section>
    <section id="collection"><div class="section-heading"><div><p class="eyebrow">the collection</p><h2>old favorites. new evenings.</h2></div><span>01 — 08</span></div>
      <div class="game-grid">${Object.entries(GAMES).map(([key, game], index) => `<button class="game-card game-${key}" data-game="${key}"><span class="card-top">${String(index + 1).padStart(2, "0")} / ${escape(game.tag)} <b>↗</b></span><span class="game-symbol" aria-hidden="true">${game.symbol}</span><span class="card-bottom"><span><strong>${escape(game.name)}</strong><small>${escape(game.description)}</small></span><i>${escape(game.players)}</i></span></button>`).join("")}</div>
    </section>
    <section id="start-panel" class="panel start-panel"><div><p class="eyebrow">a seat with your name on it</p><h2>start something good.</h2><p>sessions save as you play. invitation links carry no private player tokens.</p></div><form id="new-session-form"><label>your name<input name="display_name" maxlength="32" required pattern="[A-Za-z0-9 ._-]+" autocomplete="nickname" value="player one"></label><label>your game<select name="game_type">${Object.entries(GAMES).map(([id, game]) => `<option value="${id}">${escape(game.name)} · ${escape(game.players)}</option>`).join("")}</select></label><button class="primary" type="submit">open a table <span>→</span></button></form></section>
    <section class="saved-games"><div class="section-heading"><h2>your open tables</h2><span>${saved.length} saved in this browser</span></div>${saved.length ? `<div class="saved-list">${saved.map((session) => `<article class="saved-session"><span class="saved-symbol">${GAMES[session.game_type].symbol}</span><span><strong>${escape(GAMES[session.game_type].name)}</strong><small>${escape(session.label || "saved game")}</small></span><button class="secondary" data-resume="${escape(session.id)}">continue →</button><button class="text-button" data-forget="${escape(session.id)}" aria-label="forget saved session">forget</button></article>`).join("")}</div>` : `<p class="empty">a fresh table awaits. your saved games will appear here.</p>`}
      <details class="resume-import"><summary>have a private resume code?</summary><form id="import-form"><label>private resume code<input name="code" required autocomplete="off" spellcheck="false"></label><button type="submit" class="secondary">restore your seat</button></form></details>
    </section>
    <footer><span>tabletop</span><p>no rush. your next turn will be here.</p><span>made for a little friendly rivalry.</span></footer>`;

  document.querySelector("#new-session-form").onsubmit = createSession;
  document.querySelectorAll("[data-game]").forEach((button) => {
    button.onclick = () => {
      const form = document.querySelector("#new-session-form");
      form.elements.game_type.value = button.dataset.game;
      document.querySelector("#start-panel").scrollIntoView({ behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "center" });
      form.elements.display_name.focus({ preventScroll: true });
    };
  });
  document.querySelectorAll("[data-resume]").forEach((button) => { button.onclick = () => navigate(button.dataset.resume); });
  document.querySelectorAll("[data-forget]").forEach((button) => { button.onclick = () => forgetSession(button.dataset.forget); });
  document.querySelector("#import-form").onsubmit = restoreSession;
}

async function createSession(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const values = new FormData(form);
  formBusy(form, true);
  try {
    const access = await request("/sessions", { method: "POST", body: { game_type: values.get("game_type"), display_name: values.get("display_name").trim() } });
    saveSession(access);
    navigate(access.id);
  } catch (error) {
    formError(form, error.message);
  } finally {
    formBusy(form, false);
  }
}

async function restoreSession(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const [id, token] = new FormData(form).get("code").trim().split(".");
  if (!/^[\da-f]{8}(?:-[\da-f]{4}){3}-[\da-f]{12}$/i.test(id || "") || !/^[\w-]{43}$/.test(token || "")) return formError(form, "that resume code is incomplete.");
  formBusy(form, true);
  try {
    const session = await request(`/sessions/${id}`, { token });
    saveSession({ ...session, access_token: token });
    navigate(id);
  } catch (error) {
    formError(form, error.message);
  } finally {
    formBusy(form, false);
  }
}

function renderJoin(id) {
  app.innerHTML = `<section class="panel join-panel"><div class="join-symbol">↗</div><p class="eyebrow">you’re invited</p><h1>there’s a seat<br><em>for you.</em></h1><p>join with your own browser. this browser receives a private resume token for your seat.</p><form id="join-form"><label>your name<input name="display_name" maxlength="32" required pattern="[A-Za-z0-9 ._-]+" value="player two" autocomplete="nickname"></label><button class="primary" type="submit">take a seat →</button></form><a href="/" class="back-link">← all games</a></section>`;
  document.querySelector("#join-form").onsubmit = async (event) => {
    event.preventDefault();
    const form = event.currentTarget;
    formBusy(form, true);
    try {
      const access = await request(`/sessions/${id}/join`, { method: "POST", body: { display_name: new FormData(form).get("display_name").trim() } });
      saveSession(access);
      navigate(access.id);
    } catch (error) {
      formError(form, error.message);
    } finally {
      formBusy(form, false);
    }
  };
}

function receive(session) {
  if (!currentSession || session.state_version > currentSession.state_version) {
    const state = session.state.state;
    if (!(session.game_type === "battleship" && state.phase === "setup" && !state.ready[playerIndex(session)])) draft = {};
  }
  currentSession = session;
  renderSession();
}

function renderSession() {
  if (!currentSession) return;
  const game = GAMES[currentSession.game_type];
  const state = currentSession.state.state;
  const you = playerIndex(currentSession);
  if (document.querySelector("#session-root")?.dataset.id !== currentSession.id) {
    disposeTable();
    app.innerHTML = `<section id="session-root" data-id="${escape(currentSession.id)}"><div class="game-topbar"><a href="/" class="back-link">← the collection</a><span class="session-saved"><span class="live-dot"></span> saved as you play</span></div><div class="game-heading"><div><p class="eyebrow">${escape(game.players)} · a seat for ${escape(currentSession.you.display_name)}</p><h1>${escape(game.name)}</h1></div><div id="participants" class="participants"></div></div><div id="invite"></div><div class="game-layout"><section class="table-wrap"><div class="table-toolbar"><span id="table-status"></span><span><button id="view-top" class="table-button" aria-pressed="false">top view</button><button id="view-orbit" class="table-button" aria-pressed="false">rotate</button></span></div><div id="game-canvas" class="game-canvas ${currentSession.game_type === "battleship" ? "fleet-canvas" : ""}"></div><p id="graphics-error" class="graphics-error" role="alert" hidden></p><div class="table-bottom"><span>rendered with Three.js</span><span>click a piece. make your move.</span></div></section><aside id="game-sidebar" class="game-sidebar"></aside></div><section class="below-table"><details><summary>how to play ${escape(game.name)}</summary><p>${escape(game.rules)}</p><p id="engine-rules"></p></details><details><summary>save your seat on another browser</summary><p>this private code grants this seat, including hidden cards. keep it to yourself.</p><button id="copy-private" class="secondary">copy private resume code</button></details></section></section>`;
    document.querySelector("#view-top").onclick = (event) => {
      const active = event.currentTarget.getAttribute("aria-pressed") !== "true";
      event.currentTarget.setAttribute("aria-pressed", String(active));
      tableView.top = active;
      table?.setTop(active);
    };
    document.querySelector("#view-orbit").onclick = (event) => {
      const active = event.currentTarget.getAttribute("aria-pressed") !== "true";
      event.currentTarget.setAttribute("aria-pressed", String(active));
      tableView.orbit = active;
      table?.setOrbit(active);
    };
    document.querySelector("#copy-private").onclick = () => copyResumeCode();
  }

  ensureTable(currentSession.id);

  const status = statusText(currentSession);
  document.querySelector("#table-status").textContent = status;
  announcer.textContent = status;
  document.querySelector("#participants").innerHTML = currentSession.participants.map((player) => `<span class="participant ${player.id === currentSession.you.id ? "is-you" : ""}"><i class="seat-dot seat-${player.player_index}"></i>${escape(player.display_name)}${player.id === currentSession.you.id ? " · you" : ""}</span>`).join("");

  const invite = document.querySelector("#invite");
  if (currentSession.status === "lobby") {
    const link = `${location.origin}/?session=${currentSession.id}`;
    invite.innerHTML = `<section class="invite"><div><strong>${currentSession.participants.length} / ${game.max} seats filled</strong><p>${currentSession.game_type === "clue" ? "invite your group. the host can begin with three to six players." : "share the invitation. the game begins when the second player joins."}</p></div><div class="invite-link"><input readonly aria-label="public invitation link" value="${escape(link)}"><button id="copy-invite" class="secondary">copy invite</button></div>${currentSession.game_type === "clue" && you === 0 ? `<button id="start-clue" class="primary" ${currentSession.participants.length < 3 || mutationInFlight ? "disabled" : ""}>everyone’s here · start</button>` : ""}</section>`;
    document.querySelector("#copy-invite").onclick = () => copyText(link, "invitation copied.");
    document.querySelector("#start-clue")?.addEventListener("click", () => mutate("start"));
  } else {
    invite.innerHTML = "";
  }

  document.querySelector("#engine-rules").textContent = Array.isArray(state.rules) ? state.rules.join(" ") : state.rules || "";
  const model = boardModel(currentSession, draft, compactBoard);
  table?.update(model);
  renderSidebar(model);
}

function statusText(session) {
  const state = session.state.state;
  if (session.status === "lobby") return "a little company is on the way";
  if (session.status === "complete" || state.won) {
    if (state.won) return "beautifully played. you cleared the table.";
    if (state.winner != null) return `${typeof state.winner === "string" ? state.winner : playerName(state.winner)} wins`;
    return state.draw ? "a well-matched draw" : "the game is complete";
  }
  if (session.game_type === "solitaire") return `${state.moves} moves · your moment of quiet`;
  if (session.game_type === "battleship" && state.phase === "setup") return state.ready[playerIndex(session)] ? "fleet locked · waiting for your opponent" : "place your fleet";
  if (session.game_type === "clue" && state.pending_refuter != null) return `${playerName(state.pending_refuter)} must show a card`;
  const turn = session.game_type === "checkers" ? (state.side_to_move === "red" ? 0 : 1) : state.turn;
  return `${turn === playerIndex(session) ? "your turn" : `${playerName(turn)}’s turn`}${state.in_check ? " · check" : ""}`;
}

function playerName(index) {
  return currentSession?.participants.find((player) => player.player_index === index)?.display_name || `player ${Number(index) + 1}`;
}

function renderSidebar(model) {
  const sidebar = document.querySelector("#game-sidebar");
  const state = currentSession.state.state;
  const game = currentSession.game_type;
  const you = playerIndex(currentSession);
  const priorChoice = document.querySelector("#board-choice")?.value;
  const keyboardWasOpen = document.querySelector("#keyboard-controls")?.open;
  let controls = "";

  if (game === "chess") {
    const canClaimAfterMove = draft.from && state.draw_claim_moves?.some((move) => move.startsWith(draft.from));
    controls = `<p>${draft.from ? `selected ${escape(draft.from)}. choose a highlighted destination.` : "select one of your pieces to see its legal moves."}</p><label>promote a pawn to<select id="promotion"><option value="q">queen</option><option value="r">rook</option><option value="b">bishop</option><option value="n">knight</option></select></label>${state.can_claim_draw ? `<button class="secondary" data-command="claim_draw">claim draw</button>` : ""}${canClaimAfterMove ? `<label class="check-label"><input id="claim-after-move" type="checkbox"> claim a draw if the selected move earns it</label>` : ""}`;
  } else if (game === "checkers") {
    controls = `<p>select your piece, then every landing square.</p><p class="selection-readout">${draft.from != null ? [draft.from, ...(draft.path || [])].join(" → ") : "your move begins on the board"}</p><button id="submit-checkers" class="primary" ${!draft.path?.length ? "disabled" : ""}>submit move</button><button id="clear-draft" class="secondary">clear path</button>`;
  } else if (game === "solitaire") {
    controls = `<p>${draft.from ? `selected ${draft.count} card${draft.count === 1 ? "" : "s"}. choose a destination.` : "select a face-up card or sequence, then its destination."}</p><button class="primary" data-command="draw">${state.stock.length ? "draw a card" : "recycle waste"}</button><button id="clear-draft" class="secondary">clear selection</button><p class="score-row"><span>foundations</span><strong>${state.foundations.reduce((total, pile) => total + pile.length, 0)} / 52</strong></p>`;
  } else if (game === "connect_four") {
    controls = `<p>choose a column. four together takes the game.</p><div class="column-buttons">${Array.from({ length: 7 }, (_, column) => `<button data-column="${column}" aria-label="drop in column ${column + 1}" ${state.turn !== you || !state.legal_moves?.includes(column) ? "disabled" : ""}>${column + 1}</button>`).join("")}</div>`;
  } else if (game === "reversi") {
    controls = `<p>the lighter squares are your legal moves.</p><p class="score-row"><span>black</span><strong>${state.board.filter((piece) => piece === 1).length}</strong><span>white</span><strong>${state.board.filter((piece) => piece === 2).length}</strong></p>${state.turn === you && state.can_pass ? `<button class="primary" data-command="pass">pass turn</button>` : ""}`;
  } else if (game === "tic_tac_toe") {
    controls = `<p>you play ${you === 0 ? "X" : "O"}. choose an empty square and make your line.</p>`;
  } else if (game === "battleship") {
    controls = battleshipControls(state, you);
  } else if (game === "clue") {
    controls = clueControls(state, you);
  }

  const seen = new Set();
  keyboardChoices = [...model.tiles, ...model.pieces]
    .filter((item) => item.pick && (item.label || item.text))
    .filter((item) => {
      const key = JSON.stringify(item.pick);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  const active = currentSession.status === "active" && !mutationInFlight;
  sidebar.innerHTML = `<p class="eyebrow">at your seat</p><h2>${escape(statusText(currentSession))}</h2><div id="game-actions">${controls}</div><p id="connection-status" class="connection-status"></p><details id="keyboard-controls" ${keyboardWasOpen ? "open" : ""}><summary>keyboard board controls</summary><label>choose a space or card<select id="board-choice">${keyboardChoices.map((item, index) => `<option value="${index}">${escape(item.label || item.text)}</option>`).join("")}</select></label><button id="board-pick" class="secondary">select on board</button><p class="hint">select a source, then a destination. the same rules apply to the 3D table.</p></details>`;
  sidebar.querySelectorAll("#game-actions button, #game-actions select, #board-pick, #board-choice").forEach((control) => { control.disabled = !active; });
  const choice = document.querySelector("#board-choice");
  if (priorChoice && choice.options[Number(priorChoice)]) choice.value = priorChoice;
  document.querySelector("#board-pick").onclick = () => handlePick(keyboardChoices[Number(choice.value)]?.pick);
  document.querySelectorAll("[data-command]").forEach((button) => { button.onclick = () => submitAction({ kind: button.dataset.command }); });
  document.querySelectorAll("[data-column]").forEach((button) => { button.onclick = () => submitAction({ kind: "drop", column: Number(button.dataset.column) }); });
  document.querySelector("#submit-checkers")?.addEventListener("click", () => submitAction({ from: draft.from, path: draft.path }));
  document.querySelector("#clear-draft")?.addEventListener("click", () => { draft = {}; renderSession(); });
  document.querySelector("#random-fleet")?.addEventListener("click", () => { draft.ships = randomFleet(); renderSession(); });
  document.querySelector("#reset-fleet")?.addEventListener("click", () => { draft.ships = []; renderSession(); });
  document.querySelector("#rotate-fleet")?.addEventListener("click", () => { draft.horizontal = draft.horizontal === false; renderSession(); });
  document.querySelector("#confirm-fleet")?.addEventListener("click", () => submitAction({ kind: "place_fleet", ships: draft.ships }));
  document.querySelector("#suggest-form")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const values = new FormData(event.currentTarget);
    submitAction({ kind: "suggest", suspect: Number(values.get("suspect")), weapon: Number(values.get("weapon")) });
  });
  document.querySelector("#accuse-form")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.currentTarget;
    if (!form.querySelector("[name=acknowledge]").checked) return;
    const values = new FormData(form);
    submitAction({ kind: "accuse", suspect: Number(values.get("suspect")), weapon: Number(values.get("weapon")), room: Number(values.get("room")) });
  });
  document.querySelectorAll("[data-refute]").forEach((button) => { button.onclick = () => submitAction({ kind: "refute", card: Number(button.dataset.refute) }); });
  document.querySelectorAll("[data-note]").forEach((input) => {
    input.onchange = () => {
      try {
        localStorage.setItem(`tabletop.note.${currentSession.id}.${currentSession.you.id}.${input.dataset.note}`, String(input.checked));
      } catch {
        toast("notes can’t be saved in this browser.", true);
      }
    };
  });
}

function battleshipControls(state, you) {
  if (state.phase !== "setup") return `<p>your fleet is on the ${compactBoard ? "upper" : "left"} board. fire into the ${compactBoard ? "lower" : "right"} board.</p><p class="legend">○ miss &nbsp; ● hit &nbsp; ◆ sunk</p>`;
  if (state.ready[you]) return `<p>your fleet is locked in. the opponent cannot see your ships.</p>`;
  const count = draft.ships?.length || 0;
  const length = state.fleet_lengths[count];
  return `<p>${count === 5 ? "your five ships are placed. lock them in when you’re ready." : `place your ${length}-cell ship by clicking its starting square on your board.`}</p><p class="selection-readout">${count} / 5 ships · ${draft.horizontal === false ? "vertical" : "horizontal"}</p><button id="rotate-fleet" class="secondary">rotate next ship</button><button id="random-fleet" class="secondary">shuffle a fleet</button><button id="reset-fleet" class="secondary">clear fleet</button><button id="confirm-fleet" class="primary" ${count !== 5 ? "disabled" : ""}>lock in fleet</button>`;
}

function clueControls(state, you) {
  if (currentSession.status === "lobby") return `<p>gather three to six players. each player receives a private hand when the host starts.</p><p class="hint">quick-table edition · original room map, movement without dice.</p>`;
  const cardName = (id) => state.cards?.find((card) => card.id === id)?.name || `card ${id}`;
  const options = (items) => (items || []).map((item, index) => `<option value="${typeof item === "object" ? item.id : index}">${escape(typeof item === "object" ? item.name : item)}</option>`).join("");
  const choicePair = `<label>suspect<select name="suspect">${options(state.suspects)}</select></label><label>weapon<select name="weapon">${options(state.weapons)}</select></label>`;
  let html = `<div class="private-hand"><h3>your private hand</h3><div>${(state.your_hand || []).map((id) => `<span class="hand-card">${escape(cardName(id))}</span>`).join("")}</div></div>`;
  if (state.eliminated?.[you]) html += `<p class="notice">your accusation was wrong. you still refute suggestions, but cannot win.</p>`;
  if (state.suggestion) {
    const suggestion = state.suggestion;
    html += `<div class="suggestion"><h3>latest suggestion</h3><p>${escape(playerName(suggestion.player))}: ${escape(cardName(suggestion.suspect))}, ${escape(cardName(suggestion.weapon + 6))}, ${escape(cardName(suggestion.room + 12))}.</p><p>${escape(String(suggestion.status).replaceAll("_", " "))}</p>${state.shown_card != null ? `<p class="private-reveal">shown only to you: <strong>${escape(cardName(state.shown_card))}</strong></p>` : ""}</div>`;
  }
  if (state.pending_refuter === you) html += `<h3>show one card privately</h3>${state.refutable_cards.map((id) => `<button class="primary" data-refute="${id}">${escape(cardName(id))}</button>`).join("")}`;
  if (state.can_move) html += `<p>choose a highlighted room or corridor. you may move up to two connections.</p>`;
  if (state.can_suggest) html += `<form id="suggest-form" class="compact-form"><h3>make a suggestion</h3>${choicePair}<button type="submit" class="primary">suggest in this room</button></form>`;
  if (state.can_end_turn) html += `<button class="secondary" data-command="end_turn">end your turn</button>`;
  if (state.can_accuse) html += `<details class="accusation"><summary>make a final accusation</summary><form id="accuse-form" class="compact-form">${choicePair}<label>room<select name="room">${options(state.rooms)}</select></label><label class="check-label"><input type="checkbox" name="acknowledge" required> i understand: a wrong accusation ends my chance to win.</label><button class="danger-button" type="submit">make final accusation</button></form></details>`;
  html += `<details class="detective-notes"><summary>your detective notebook</summary><p class="hint">private checkmarks stay in this browser.</p>${(state.cards || []).map((card) => {
    let checked = state.your_hand?.includes(card.id);
    try { const saved = localStorage.getItem(`tabletop.note.${currentSession.id}.${currentSession.you.id}.${card.id}`); if (saved != null) checked = saved === "true"; } catch { /* hand is the fallback */ }
    return `<label class="check-label"><input type="checkbox" data-note="${card.id}" ${checked ? "checked" : ""}>${escape(card.name)}</label>`;
  }).join("")}</details>`;
  return html;
}

function handlePick(pick) {
  if (!pick || !currentSession || mutationInFlight || currentSession.status !== "active") return;
  const state = currentSession.state.state;
  const you = playerIndex(currentSession);
  if (pick.kind === "chess") {
    if (state.turn !== you) return;
    const promotion = document.querySelector("#promotion")?.value || "q";
    const move = draft.from && state.legal_moves.find((candidate) => candidate.startsWith(`${draft.from}${pick.square}`) && (candidate.length === 4 || candidate[4] === promotion));
    if (move) {
      const claimed = document.querySelector("#claim-after-move")?.checked && state.draw_claim_moves?.includes(move);
      return submitAction({ kind: claimed ? "claim_draw_after_move" : "move", from: draft.from, to: pick.square, promotion: move[4] || null });
    }
    draft = state.legal_moves.some((candidate) => candidate.startsWith(pick.square)) ? { from: pick.square } : {};
  } else if (pick.kind === "checkers") {
    if ((state.side_to_move === "red" ? 0 : 1) !== you) return;
    const piece = state.board[pick.square];
    const own = you === 0 ? piece === 1 || piece === 2 : piece === 3 || piece === 4;
    if (own) draft = draft.from === pick.square ? {} : { from: pick.square, path: [] };
    else if (draft.from != null && !piece) draft.path.push(pick.square);
  } else if (pick.kind === "draw") {
    return submitAction({ kind: "draw" });
  } else if (pick.kind === "solitaire") {
    if (draft.from && !samePile(draft.from, pick.to) && pick.to.kind !== "waste") return submitAction({ kind: "move", from: draft.from, to: pick.to, count: draft.count });
    draft = pick.from ? { from: pick.from, count: pick.count } : {};
  } else if (["connect_four", "reversi", "tic_tac_toe"].includes(pick.kind)) {
    if (state.turn !== you) return;
    return submitAction(pick.kind === "connect_four" ? { kind: "drop", column: pick.column } : { kind: "place", square: pick.square });
  } else if (pick.kind === "battleship") {
    if (state.phase === "setup" && pick.own && !state.ready[you]) {
      const ships = [...(draft.ships || []), { row: pick.row, column: pick.column, horizontal: draft.horizontal !== false }];
      if (ships.length > 5) return toast("all five ships are placed. lock in or clear the fleet.");
      if (!fleetCells(ships)) return toast("that ship overlaps another or runs past the edge.", true);
      draft.ships = ships;
    } else if (state.phase === "playing" && !pick.own && state.turn === you) {
      return submitAction({ kind: "fire", row: pick.row, column: pick.column });
    } else return;
  } else if (pick.kind === "clue") {
    if (state.can_move && state.legal_destinations.includes(pick.destination)) return submitAction({ kind: "move", destination: pick.destination });
    return;
  }
  renderSession();
}

function submitAction(action) {
  return mutate("actions", {
    expected_version: currentSession.state_version,
    action: { game_type: currentSession.game_type, action },
  });
}

async function mutate(path, body) {
  if (!currentSession || mutationInFlight) return;
  const session = currentSession;
  const saved = savedSessions()[session.id];
  if (!saved) return;
  const generation = responses.generation;
  mutationInFlight = true;
  stopPolling();
  renderSession();
  try {
    const response = await request(`/sessions/${session.id}/${path}`, { method: "POST", body, token: saved.access_token });
    if (responses.accepts(generation, response)) receive(response);
  } catch (error) {
    if (generation === responses.generation) {
      toast(error.message, true);
      if (error.status === 409 || error.status === 422) {
        try {
          const fresh = await request(`/sessions/${session.id}`, { token: saved.access_token });
          if (responses.accepts(generation, fresh)) receive(fresh);
        } catch {
          // The original request error is already visible and is the useful signal.
        }
      }
    }
  } finally {
    if (generation === responses.generation) {
      mutationInFlight = false;
      renderSession();
      schedulePoll();
    }
  }
}

function schedulePoll() {
  stopPolling();
  if (!currentSession || currentSession.status === "complete" || currentSession.game_type === "solitaire") return;
  pollTimer = setTimeout(poll, 3000);
}

async function poll() {
  if (!currentSession || mutationInFlight) return schedulePoll();
  const session = currentSession;
  const token = savedSessions()[session.id]?.access_token;
  const generation = responses.generation;
  if (!token) return;
  try {
    const fresh = await request(`/sessions/${session.id}`, { token, signal: routeController.signal });
    if (generation !== responses.generation) return;
    if (responses.accepts(generation, fresh) && fresh.state_version > currentSession.state_version) receive(fresh);
    const status = document.querySelector("#connection-status");
    if (status) status.textContent = "";
  } catch (error) {
    if (generation !== responses.generation || error.name === "AbortError") return;
    const status = document.querySelector("#connection-status");
    if (status) status.textContent = "connection interrupted. reconnecting…";
  } finally {
    if (generation === responses.generation) schedulePoll();
  }
}

function formBusy(form, busy) {
  form.querySelectorAll("button, input, select").forEach((control) => { control.disabled = busy; });
}

function formError(form, message) {
  form.querySelector(".form-error")?.remove();
  const error = document.createElement("p");
  error.className = "form-error";
  error.setAttribute("role", "alert");
  error.textContent = message;
  form.append(error);
}

function graphicsError(message) {
  const target = document.querySelector("#graphics-error");
  if (!target) return;
  target.hidden = false;
  target.textContent = message;
}

function toast(message, danger = false, duration = 5500) {
  document.querySelector(".toast")?.remove();
  const node = document.createElement("div");
  node.className = `toast${danger ? " danger" : ""}`;
  node.setAttribute("role", "status");
  node.textContent = message;
  document.body.append(node);
  setTimeout(() => node.remove(), duration);
}

async function copyText(value, message) {
  try {
    await navigator.clipboard.writeText(value);
    toast(message);
  } catch {
    const field = document.createElement("textarea");
    field.value = value;
    field.className = "copy-fallback";
    field.setAttribute("aria-label", "select and copy this value");
    document.body.append(field);
    field.select();
    toast("clipboard access is unavailable. copy the selected text, then press escape.");
    field.addEventListener("keydown", (event) => { if (event.key === "Escape") field.remove(); });
  }
}

function copyResumeCode() {
  const token = savedSessions()[currentSession.id]?.access_token;
  if (token) copyText(`${currentSession.id}.${token}`, "private resume code copied—keep it to yourself.");
}

document.addEventListener("click", (event) => {
  const link = event.target.closest('a[href="/"]');
  if (link && !event.metaKey && !event.ctrlKey && !event.shiftKey && event.button === 0) {
    event.preventDefault();
    navigate();
  }
});
window.addEventListener("popstate", renderRoute);
window.addEventListener("resize", () => {
  const nextCompact = window.innerWidth < 800;
  if (nextCompact !== compactBoard) {
    compactBoard = nextCompact;
    if (currentSession) renderSession();
  }
});
window.addEventListener("pagehide", () => {
  stopPolling();
  routeController?.abort();
  disposeTable();
});

renderRoute();
