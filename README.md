# Scrabble

A shared-board and private-phone multiplayer Scrabble application, under construction. The browser currently presents a public board preview; the browser transport adapter is verified against a local game-server runtime, while the Scrabble simulation adapter now runs inside that runtime, and a Rust executable now hosts independent matches with runtime health, readiness and graceful recovery.

## Development

Install Bun 1.4.2 and Rust (the toolchain file selects 1.98.1 with Clippy and rustfmt). From a fresh checkout:

```sh
bun install --frozen-lockfile
cargo fetch --locked
bunx playwright install --with-deps chromium
bun run check
```

`bun run dev` opens the browser development server; visit its printed URL under `/scrabble/`. `bun run dev:tls` provisions a local TLS identity, and `bun run dev:server` starts the authoritative hosted server. See [server setup and configuration](docs/specs/server-executable.md) for local ports, production requirements, and graceful restart.

`bun run check` checks formatting, TypeScript, lint, Rust tests, browser rendering, the static production build, and Chromium layout and real loopback WebTransport checks. `bun run format` applies formatting. Dependency changes use `bun install` and commit `bun.lock`; Rust dependency changes include `Cargo.lock`.

## Hosting

The browser uses `/scrabble/` as its Vite base, including in development. GitHub Actions validates and builds once, uploads `dist`, and deploys that artifact on main to GitHub Pages. Configure repository Pages to use GitHub Actions. No server credential belongs in this public static bundle. Production authoritative hosting is tracked in #31.

`cargo` owns canonical game logic under `crates/scrabble-game`; `crates/scrabble-server` adapts it to `game-server` through `ScrabbleSimulation`. `apps/web` owns presentation only. See the ordered GitHub issues #1–#34 for the MVP acceptance requirements.

The versioned domain contracts and offline English fixture ruleset are defined in `crates/scrabble-game`; see [protocol v1](docs/specs/protocol-v1.md) and the [domain glossary](CONTEXT.md). The fixture dictionary is test-only, not a full playable word list.

`GameState` provides deterministic tile storage, initial dealing, replacement draws, and scoped projections. [State storage](docs/specs/domain-state.md) documents the seeded initialization contract and the validation still required before gameplay commits.

`GameSession::apply` now provides the [authoritative turn boundary](docs/specs/turn-application.md) for start, preview/cancel, and pass, with identity, sequence and turn checks. Commit and exchange remain unavailable until their ordered rule/transaction slices.

[Placement validation](docs/specs/placement-validation.md) derives the main and cross words through a pure borrowed-state seam, with structured structural errors. Dictionary acceptance and scoring remain separate checks.

[Scoring](docs/specs/scoring.md) calculates all validated words with new-tile premiums and a single full-rack bonus. The scored storage commit applies cumulative points atomically; browser command integration remains in #19.

The [shared board](docs/specs/shared-board.md) renders public snapshot fixtures for lobby, playing, and finished phases. Add `?fixture=playing` or `?fixture=finished` to the `/scrabble/` entry point to inspect those presentation cases; multiplayer is not connected yet. `bun run test:browser` runs focused layout checks and writes screenshots to ignored `test-results/`.

The [browser transport](docs/specs/browser-transport.md) supports versioned welcome, bounded commands, latest-state snapshot reassembly, reliable control and in-memory reconnect capabilities. Its real-network fixture exercises upstream `game-server` independently of Scrabble rules. Production endpoint configuration remains #31.

The [simulation adapter](docs/specs/simulation-adapter.md) maps runtime admissions directly to domain identities, applies authenticated commands, keeps ticks independent of turns, and separates canonical replay evidence from private player projections. Browser-free tests verify runtime replay, recovery, reconnect fencing and grace expiry. Started-game forfeit resolution remains #27.

The [authoritative executable](docs/specs/server-executable.md) reuses game-server hosting/status/recovery for independent named matches. Private persistent initialization keeps tile order reproducible across restart. The real-process test proves scoped racks, match isolation, SIGTERM recovery and reconnect sequence fencing; a complete playable MVP still requires the remaining issues.
