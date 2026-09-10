# tabletop

a small, rust-backed web experience for persistent games, rendered as interactive 3D tables with Three.js:

- **solitaire:** one player, server-validated Klondike moves, resumable from any browser that retains its private access token.
- **chess:** two players, legal move generation, castling, en passant, promotion, checkmate, and claimable or automatic draws.
- **battleship:** two players, private fleets, server-side placement validation, and redacted opponent waters.
- **Clue:** three to six players, a compact original room map, private hands and refutations, suggestions, accusations, and elimination.
- **checkers:** two players, forced captures, atomic multi-jumps, and promotion.
- **connect four, reversi, and tic-tac-toe:** two-player server-authoritative classics.

## architecture

the application has three deployable tiers:

1. **web:** Vite bundles a native ES-module Three.js client; nginx serves static frontend content while ingress routes `/api` directly to the API service.
2. **api:** an axum service owns validation and every state transition. replicas retain no game state.
3. **database:** PostgreSQL stores session snapshots, append-only action events, and hashed 256-bit per-seat bearer tokens.

the API locks one session row while it validates and saves an action, then advances its version and writes the matching event in the same transaction. that keeps every game action correct even when requests land on different API pods. Per-player state views redact hidden Battleship fleets, Clue hands, refutation choices, and private reveals before a response leaves the API.

`src/domain/` contains pure game rules and isolated state/action variants. HTTP, persistence, session membership, optimistic state versions, and client session mechanics stay shared.

## local development

you need Rust 1.90+, PostgreSQL 17+, and a `DATABASE_URL` such as:

```sh
export DATABASE_URL='postgresql://board_games:local-dev-password@localhost:5432/board_games?sslmode=disable'
cargo run -- migrate
cargo run -- serve
```

in another terminal, install the pinned browser dependencies and use Vite's same-origin API proxy:

```sh
npm ci
npm run dev
```

Set `API_PROXY_TARGET` to override Vite's local development proxy target when the API is not listening on `127.0.0.1:8080`. This setting affects local Vite development only; the production web container does not use it.

## k3s deployment

The GitHub Actions workflow in `.github/workflows/container-image.yml` builds and publishes both images to GHCR on pushes to `develop` and version tags. It gives each image commit-derived `sha-*` tags; use the resulting immutable image digests in `deploy/k8s/kustomization.yaml` for a registry-backed deployment. Do not deploy `latest` in a real environment.

The checked-in manifests pin both application images to immutable registry digests in `deploy/k8s/kustomization.yaml`. After publishing, update both digests to the matching images’ resolved registry digests before applying the manifests.

create the secret from `deploy/k8s/secret.example.yaml` **outside this repository** after replacing both placeholders with the same strong random password. This cluster serves the app at `games.omv.mousses.xyz`; other clusters should replace that host in `deploy/k8s/ingress.yaml` and the Certificate resources.

The GHCR packages are private, so create the image pull Secret outside this repository before applying the kustomization. Use a GitHub classic PAT with `read:packages`:

```sh
kubectl -n board-games create secret docker-registry ghcr-pull \
  --docker-server=ghcr.io \
  --docker-username=petermousses \
  --docker-password="$CR_PAT"
```

then run the ordered migration-first deployment:

```sh
kubectl apply -f /secure/path/board-games-secrets.yaml
scripts/deploy-k8s.sh
```

The manifest deliberately keeps the API rollout paused. The script refuses an active or failed prior migration Job, removes only a completed one, applies the manifests, waits for the bounded migration Job, then resumes and verifies the API and web rollouts. API readiness validates every embedded SQLx migration against `_sqlx_migrations`, including success and checksum, before accepting traffic.

## verification

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
npm ci
npm run build
npm test
kubectl kustomize deploy/k8s
```
