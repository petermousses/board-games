const API_ROOT = "/api/v1";
const SAVED_SESSIONS_KEY = "tabletop.saved-sessions.v1";

let currentSession = null;
let checkersDraft = null;
let pollTimer = null;

const app = document.querySelector("#app");

function savedSessions() {
  try {
    return JSON.parse(localStorage.getItem(SAVED_SESSIONS_KEY)) ?? {};
  } catch {
    return {};
  }
}

function saveSession(access) {
  const sessions = savedSessions();
  sessions[access.id] = {
    id: access.id,
    access_token: access.access_token,
    game_type: access.game_type,
    label: access.participants?.find((player) => player.id === access.you.id)?.display_name ?? "saved game",
    saved_at: new Date().toISOString(),
  };
  localStorage.setItem(SAVED_SESSIONS_KEY, JSON.stringify(sessions));
}

function forgetSession(id) {
  const sessions = savedSessions();
  delete sessions[id];
  localStorage.setItem(SAVED_SESSIONS_KEY, JSON.stringify(sessions));
  renderHome();
}

async function request(path, options = {}, accessToken) {
  const headers = { "Content-Type": "application/json", ...(options.headers ?? {}) };
  if (accessToken) headers.Authorization = `Bearer ${accessToken}`;
  const response = await fetch(`${API_ROOT}${path}`, { ...options, headers });
  if (response.status === 204) return null;
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error ?? `request failed (${response.status})`);
  return body;
}

function navigateToSession(id) {
  window.history.pushState({}, "", `/?session=${encodeURIComponent(id)}`);
  renderRoute();
}

function sessionState(session) {
  return session.state.state;
}

function renderHome() {
  stopPolling();
  const sessions = Object.values(savedSessions()).sort((a, b) => b.saved_at.localeCompare(a.saved_at));
  app.innerHTML = `
    <section class="hero">
      <p class="eyebrow">your next turn is waiting</p>
      <h1>pick a game.<br>keep the session.</h1>
      <p class="lede">the board lives in the database, not one browser tab. close this, return later, keep playing.</p>
    </section>
    <section class="panel game-start">
      <h2>start a session</h2>
      <form id="new-session-form" class="form-grid">
        <label>name <input required maxlength="32" name="display_name" value="player one" autocomplete="nickname"></label>
        <label>game
          <select name="game_type">
            <option value="solitaire">solitaire · single player</option>
            <option value="checkers">checkers · two players</option>
          </select>
        </label>
        <button class="primary" type="submit">start game</button>
      </form>
      <p class="hint">checkers starts a red seat, then gives you a shareable join link. your private resume token never enters that link.</p>
    </section>
    <section class="saved-games">
      <div class="section-heading"><h2>continue a session</h2><span>${sessions.length} saved here</span></div>
      ${sessions.length ? `<div class="session-list">${sessions.map(savedSessionCard).join("")}</div>` : `<p class="empty">nothing saved in this browser yet.</p>`}
    </section>
  `;

  document.querySelector("#new-session-form").addEventListener("submit", startSession);
  document.querySelectorAll("[data-open-session]").forEach((button) => {
    button.addEventListener("click", () => navigateToSession(button.dataset.openSession));
  });
  document.querySelectorAll("[data-forget-session]").forEach((button) => {
    button.addEventListener("click", () => forgetSession(button.dataset.forgetSession));
  });
}

function savedSessionCard(session) {
  return `
    <article class="saved-session">
      <div><span class="game-pill">${escapeHtml(session.game_type)}</span><h3>${escapeHtml(session.label)}</h3></div>
      <div class="row-actions">
        <button class="secondary" data-open-session="${session.id}">continue</button>
        <button class="text-button" data-forget-session="${session.id}" aria-label="forget saved session">forget</button>
      </div>
    </article>`;
}

async function startSession(event) {
  event.preventDefault();
  const form = new FormData(event.currentTarget);
  setFormBusy(event.currentTarget, true);
  try {
    const access = await request("/sessions", {
      method: "POST",
      body: JSON.stringify({
        game_type: form.get("game_type"),
        display_name: form.get("display_name").trim(),
      }),
    });
    saveSession(access);
    navigateToSession(access.id);
  } catch (error) {
    showFormError(event.currentTarget, error.message);
  } finally {
    setFormBusy(event.currentTarget, false);
  }
}

async function renderRoute() {
  stopPolling();
  const params = new URLSearchParams(window.location.search);
  const id = params.get("session");
  if (!id) return renderHome();

  const saved = savedSessions()[id];
  if (!saved) return renderJoin(id);
  try {
    currentSession = await request(`/sessions/${id}`, {}, saved.access_token);
    checkersDraft = null;
    renderSession(currentSession);
  } catch (error) {
    app.innerHTML = `<section class="panel error-panel"><h1>can’t open this session</h1><p>${escapeHtml(error.message)}</p><a class="secondary link-button" href="/">back home</a></section>`;
  }
}

function renderJoin(id) {
  app.innerHTML = `
    <section class="panel join-panel">
      <p class="eyebrow">checkers invitation</p>
      <h1>take the black seat</h1>
      <p>this link is intentionally public. joining gives this browser its own private resume token.</p>
      <form id="join-form" class="form-grid">
        <label>name <input required maxlength="32" name="display_name" value="player two" autocomplete="nickname"></label>
        <button class="primary" type="submit">join checkers</button>
      </form>
    </section>`;
  document.querySelector("#join-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    setFormBusy(event.currentTarget, true);
    try {
      const name = new FormData(event.currentTarget).get("display_name").trim();
      const access = await request(`/sessions/${id}/join`, {
        method: "POST",
        body: JSON.stringify({ display_name: name }),
      });
      saveSession(access);
      navigateToSession(access.id);
    } catch (error) {
      showFormError(event.currentTarget, error.message);
    } finally {
      setFormBusy(event.currentTarget, false);
    }
  });
}

function renderSession(session) {
  currentSession = session;
  const content = session.game_type === "solitaire" ? renderSolitaire(session) : renderCheckers(session);
  app.innerHTML = `
    <section class="game-topbar">
      <a href="/" class="back-link">← all games</a>
      <div><span class="game-pill">${session.game_type}</span><strong>${escapeHtml(session.status)}</strong></div>
    </section>
    ${content}`;
  if (session.game_type === "solitaire") bindSolitaire();
  if (session.game_type === "checkers") bindCheckers();
  if (session.game_type === "checkers" && session.status !== "complete") startPolling(session.id);
}

function renderSolitaire(session) {
  const state = sessionState(session);
  return `
    <section class="game-header"><div><p class="eyebrow">single player</p><h1>solitaire</h1></div><p>${state.moves} moves${state.won ? " · you cleared it" : ""}</p></section>
    <section class="solitaire-board" aria-label="solitaire board">
      <div class="solitaire-top-row">
        <button class="stock pile" data-draw>${state.stock.length ? `▧<small>${state.stock.length}</small>` : "↻"}</button>
        ${renderCardStack(state.waste, state.waste.length - 1, { kind: "waste" }, true)}
        <div class="foundation-row">${state.foundations.map((pile, index) => renderFoundation(pile, index)).join("")}</div>
      </div>
      <div class="tableau-row">${state.tableau.map((pile, index) => renderTableauPile(pile, index)).join("")}</div>
      <p class="hint board-hint">click a face-up card, then its destination. click the stock to draw or recycle.</p>
    </section>`;
}

function renderCardStack(cards, topIndex, source, destination) {
  const card = cards.at(-1);
  const sourceAttributes = card ? `data-solitaire-source='${JSON.stringify({ from: source, count: 1 })}'` : "";
  const destinationAttributes = destination ? `data-solitaire-destination='${JSON.stringify(source)}'` : "";
  return `<div class="card-slot" ${destinationAttributes}>${card ? `<button class="card ${cardColor(card)}" ${sourceAttributes}>${cardLabel(card)}</button>` : ""}</div>`;
}

function renderFoundation(cards, index) {
  const card = cards.at(-1);
  const source = { kind: "foundation", index };
  return `<div class="card-slot foundation" data-solitaire-destination='${JSON.stringify(source)}'>
    ${card ? `<button class="card ${cardColor(card)}" data-solitaire-source='${JSON.stringify({ from: source, count: 1 })}'>${cardLabel(card)}</button>` : `<span>${["♠", "♥", "♦", "♣"][index]}</span>`}
  </div>`;
}

function renderTableauPile(pile, index) {
  const destination = JSON.stringify({ kind: "tableau", index });
  return `<div class="tableau-pile" data-solitaire-destination='${destination}'>${pile.cards.map((card, cardIndex) => {
    if (cardIndex < pile.face_up_from) return `<span class="card back" style="--depth:${cardIndex}">▧</span>`;
    const count = pile.cards.length - cardIndex;
    return `<button class="card ${cardColor(card)}" style="--depth:${cardIndex}" data-solitaire-source='${JSON.stringify({ from: { kind: "tableau", index }, count })}'>${cardLabel(card)}</button>`;
  }).join("")}</div>`;
}

function bindSolitaire() {
  document.querySelector("[data-draw]").addEventListener("click", () => submitAction({ kind: "draw" }));
  document.querySelectorAll("[data-solitaire-source]").forEach((element) => {
    element.addEventListener("click", (event) => {
      event.stopPropagation();
      const selection = JSON.parse(element.dataset.solitaireSource);
      document.querySelectorAll(".card.selected").forEach((card) => card.classList.remove("selected"));
      element.classList.add("selected");
      window.solitaireSelection = selection;
    });
  });
  document.querySelectorAll("[data-solitaire-destination]").forEach((element) => {
    element.addEventListener("click", () => {
      if (!window.solitaireSelection) return;
      const to = JSON.parse(element.dataset.solitaireDestination);
      const { from, count } = window.solitaireSelection;
      window.solitaireSelection = null;
      submitAction({ kind: "move", from, to, count });
    });
  });
}

function renderCheckers(session) {
  const state = sessionState(session);
  const players = session.participants.map((player) => `${player.display_name} (${player.seat})`).join(" · ");
  const you = session.you.seat;
  return `
    <section class="game-header"><div><p class="eyebrow">multiplayer · you are ${you}</p><h1>checkers</h1></div><p>${escapeHtml(players)}</p></section>
    ${session.status === "lobby" ? renderInvite(session) : ""}
    <section class="checkers-layout">
      <div class="checkers-board" aria-label="checkers board">${renderCheckersBoard(state.board)}</div>
      <aside class="game-sidebar">
        <h2>${state.winner ? `${state.winner} wins` : `${state.side_to_move} to move`}</h2>
        <p>captures are mandatory. build a whole jump path before submitting it.</p>
        <p class="draft-path">${checkersDraft ? `path: ${[checkersDraft.from, ...checkersDraft.path].join(" → ")}` : "select a piece"}</p>
        <button class="primary" data-submit-checkers ${checkersDraft?.path.length ? "" : "disabled"}>submit move</button>
        <button class="secondary" data-clear-checkers ${checkersDraft ? "" : "disabled"}>clear path</button>
      </aside>
    </section>`;
}

function renderInvite(session) {
  const link = `${window.location.origin}/?session=${encodeURIComponent(session.id)}`;
  return `<section class="invite panel"><h2>waiting for black</h2><p>share this public join link. do not share your saved browser data.</p><code>${escapeHtml(link)}</code><button class="secondary" data-copy-invite="${link}">copy link</button></section>`;
}

function renderCheckersBoard(board) {
  const squares = [];
  for (let row = 0; row < 8; row += 1) {
    for (let column = 0; column < 8; column += 1) {
      if ((row + column) % 2 === 0) {
        squares.push('<span class="square light"></span>');
        continue;
      }
      const index = row * 4 + Math.floor(column / 2);
      const piece = board[index];
      const selected = checkersDraft && (checkersDraft.from === index || checkersDraft.path.includes(index));
      squares.push(`<button class="square dark ${selected ? "selected-square" : ""}" data-checkers-square="${index}" aria-label="square ${index}">${piece ? `<span class="piece ${piece < 3 ? "red-piece" : "black-piece"}">${piece === 2 || piece === 4 ? "♛" : ""}</span>` : ""}</button>`);
    }
  }
  return squares.join("");
}

function bindCheckers() {
  document.querySelectorAll("[data-checkers-square]").forEach((button) => {
    button.addEventListener("click", () => selectCheckersSquare(Number(button.dataset.checkersSquare)));
  });
  document.querySelector("[data-submit-checkers]")?.addEventListener("click", () => {
    if (!checkersDraft) return;
    submitAction({ from: checkersDraft.from, path: checkersDraft.path });
  });
  document.querySelector("[data-clear-checkers]")?.addEventListener("click", () => {
    checkersDraft = null;
    renderSession(currentSession);
  });
  document.querySelector("[data-copy-invite]")?.addEventListener("click", async (event) => {
    await navigator.clipboard.writeText(event.currentTarget.dataset.copyInvite);
    event.currentTarget.textContent = "copied";
  });
}

function selectCheckersSquare(square) {
  const state = sessionState(currentSession);
  const piece = state.board[square];
  const isYourPiece = currentSession.you.seat === "red" ? piece === 1 || piece === 2 : piece === 3 || piece === 4;
  if (!checkersDraft) {
    if (!isYourPiece) return;
    checkersDraft = { from: square, path: [] };
  } else if (square === checkersDraft.from) {
    checkersDraft = null;
  } else {
    checkersDraft.path.push(square);
  }
  renderSession(currentSession);
}

async function submitAction(action) {
  const saved = savedSessions()[currentSession.id];
  if (!saved) return;
  try {
    const session = await request(`/sessions/${currentSession.id}/actions`, {
      method: "POST",
      body: JSON.stringify({ action: { game_type: currentSession.game_type, action } }),
    }, saved.access_token);
    checkersDraft = null;
    renderSession(session);
  } catch (error) {
    showToast(error.message, true);
  }
}

function startPolling(id) {
  stopPolling();
  pollTimer = window.setInterval(async () => {
    const saved = savedSessions()[id];
    if (!saved || currentSession?.id !== id) return stopPolling();
    try {
      const fresh = await request(`/sessions/${id}`, {}, saved.access_token);
      if (fresh.state_version !== currentSession.state_version || fresh.status !== currentSession.status) {
        checkersDraft = null;
        renderSession(fresh);
      }
    } catch {
      // A transient network failure should not erase an active board.
    }
  }, 2000);
}

function stopPolling() {
  if (pollTimer) window.clearInterval(pollTimer);
  pollTimer = null;
}

function cardLabel(card) {
  return `${["A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K"][card % 13]}${["♠", "♥", "♦", "♣"][Math.floor(card / 13)]}`;
}

function cardColor(card) {
  return Math.floor(card / 13) === 1 || Math.floor(card / 13) === 2 ? "red-card" : "black-card";
}

function setFormBusy(form, busy) {
  form.querySelectorAll("button, input, select").forEach((field) => { field.disabled = busy; });
}

function showFormError(form, message) {
  form.querySelector(".form-error")?.remove();
  const error = document.createElement("p");
  error.className = "form-error";
  error.textContent = message;
  form.append(error);
}

function showToast(message, danger = false) {
  document.querySelector(".toast")?.remove();
  const toast = document.createElement("div");
  toast.className = `toast ${danger ? "danger" : ""}`;
  toast.textContent = message;
  document.body.append(toast);
  window.setTimeout(() => toast.remove(), 3200);
}

function escapeHtml(value) {
  return String(value).replace(/[&<>'"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;" })[character]);
}

window.addEventListener("popstate", renderRoute);
renderRoute();
