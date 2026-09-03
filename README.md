# tabletop

a small, rust-backed web experience for persistent games:

- **solitaire:** one player, server-validated klondike moves, resumable from any browser that retains its private access token.
- **checkers:** two players, invite-by-session link, server-authoritative turns, forced captures, atomic multi-jumps, and promotion.

## architecture

the application has three deployable tiers:

1. **web:** nginx serves the browser client and reverse-proxies same-origin `/api/` requests.
2. **api:** an axum service owns validation and every state transition. replicas retain no game state.
3. **database:** PostgreSQL stores session snapshots, append-only action events, and hashed 256-bit per-seat bearer tokens.

the API locks one session row while it validates and saves an action, then advances its version and writes the matching event in the same transaction. this keeps a checkers move correct even when requests land on different API pods.

`src/domain/` contains pure game rules over compact arrays: 32 playable checkers squares and 0–51 card values. Adding a game means adding a state/action variant and its isolated rules module; HTTP, persistence, and the client session mechanics stay shared.

## local development

you need Rust 1.90+, PostgreSQL 17+, and a `DATABASE_URL` such as:

```sh
export DATABASE_URL='postgresql://board_games:local-dev-password@localhost:5432/board_games?sslmode=disable'
cargo run -- migrate
cargo run -- serve
```

serve the `web/` directory through a same-origin proxy to the API for local browser work. The production web container already supplies that proxy.

## k3s deployment

The GitHub Actions workflow in `.github/workflows/container-image.yml` builds and publishes both images to GHCR on pushes to `develop` and version tags. It gives each image commit-derived `sha-*` tags; use the resulting immutable image digests in `deploy/k8s/kustomization.yaml` for a registry-backed deployment. Do not deploy `latest` in a real environment.

The checked-in manifests use the workflow’s `develop` tags as a bootstrap reference and pull them from GHCR. After publishing, update both image tags to the matching commit’s `sha-*` tag or, preferably, its resolved registry digest before applying the manifests.

create the secret from `deploy/k8s/secret.example.yaml` **outside this repository** after replacing both placeholders with the same strong random password. This cluster serves the app at `games.omv.mousses.xyz`; other clusters should replace that host in `deploy/k8s/ingress.yaml` and the Certificate resources.

then apply and watch the migration plus rollouts:

```sh
kubectl apply -f /secure/path/board-games-secrets.yaml
kubectl apply -k deploy/k8s
kubectl -n board-games rollout status deployment/board-games-api
kubectl -n board-games rollout status deployment/board-games-web
```

each API replica runs embedded migrations before it starts listening; a PostgreSQL advisory lock serializes that step. The readiness probe also checks for the migrated `game_sessions` table, so traffic stays out until migrations succeed.

## verification

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
kubectl kustomize deploy/k8s
node --check web/app.js
```

the current workstation’s macOS compiler toolchain and Docker daemon are unavailable; CI should run the Rust commands in a Linux builder before release.
