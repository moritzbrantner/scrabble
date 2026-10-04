# Scrabble

A shared-board and private-phone multiplayer Scrabble application, under construction. The development board can create authoritative games, publish copyable player links and locally rendered QR codes, and display their live public state. The Rust executable hosts independent matches with runtime health, readiness and graceful recovery. Production endpoint configuration and the remaining gameplay/player workflows are still tracked in the ordered issues.

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

The browser uses `/scrabble/` as its Vite base, including in development. GitHub Actions validates and builds once, uploads `dist`, and deploys that artifact on main to GitHub Pages. Configure repository Pages to use GitHub Actions. No server credential belongs in this public static bundle. See [production deployment](docs/specs/production-deployment.md) for the Compose topology, public endpoint configuration, and rollout procedure; an actual hosting target remains outstanding in #31.

`cargo` owns canonical game logic under `crates/scrabble-game`; `crates/scrabble-server` adapts it to `game-server` through `ScrabbleSimulation`. `apps/web` owns presentation only. See the ordered GitHub issues #1–#34 for the MVP acceptance requirements.

The versioned domain contracts and offline English fixture ruleset are defined in `crates/scrabble-game`; see [protocol v1](docs/specs/protocol-v1.md) and the [domain glossary](CONTEXT.md). The default fixture dictionary is test-only; deployments can load a separate local list through the [dictionary boundary](docs/specs/dictionary-validation.md).

`GameState` provides deterministic tile storage, initial dealing, replacement draws, and scoped projections. [State storage](docs/specs/domain-state.md) documents the seeded initialization contract and the validation still required before gameplay commits.

`GameSession::apply` now provides the [authoritative turn boundary](docs/specs/turn-application.md) for start, preview/cancel, commit, pass and exchange, with identity, sequence and turn checks. [Game completion](docs/specs/game-completion.md) applies final rack deductions, go-out transfers, and explicit ties, then freezes gameplay. [Deterministic history](docs/specs/deterministic-history.md) exposes accepted public turns and verifies private native replay evidence offline. [Mobile interaction and accessibility](docs/specs/mobile-accessibility.md) covers touch layouts, keyboard gameplay, themes, and interface languages. [Configurable rulesets](docs/specs/configurable-rulesets.md) bind deployment-owned alphabets, tile distributions, and scoring settings to immutable replay evidence.

[Placement validation](docs/specs/placement-validation.md) derives the main and cross words through a pure borrowed-state seam, with structured structural errors. Dictionary acceptance and scoring remain separate checks.

[Scoring](docs/specs/scoring.md) calculates all validated words with new-tile premiums and a single full-rack bonus. The scored storage commit applies cumulative points atomically; browser command integration remains in #19.

The [shared board](docs/specs/shared-board.md) renders public snapshot fixtures for lobby, playing, and finished phases. Add `?fixture=playing` or `?fixture=finished` to the `/scrabble/` entry point to inspect those presentation cases. Fixture previews are development-only. A production build requires the public service configuration or displays an explicit service-unavailable message. `bun run test:browser` runs focused layout checks and writes screenshots to ignored `test-results/`.

The [browser transport](docs/specs/browser-transport.md) supports versioned welcome, bounded commands, latest-state snapshot reassembly, reliable control and in-memory reconnect capabilities. Its real-network fixture exercises upstream `game-server` independently of Scrabble rules. Production endpoint configuration remains #31.

The [simulation adapter](docs/specs/simulation-adapter.md) maps runtime admissions directly to domain identities, applies authenticated commands, keeps ticks independent of turns, and separates canonical replay evidence from private player projections. Browser-free tests verify runtime replay, recovery, reconnect fencing and grace expiry. [Private client boundaries](docs/specs/private-client-boundaries.md) document scoped projections, rejected malicious commands, capability fencing, and redacted diagnostics.

The [creation API](docs/specs/game-creation.md) derives high-entropy public match IDs, bounds retries and match lifetime, rejects draining/full hosts, and retires through the upstream fencing boundary. Chromium acceptance drops a successful response, retries to recover the same game, and joins its canonical route.

The [authoritative executable](docs/specs/server-executable.md) reuses game-server hosting/status/recovery for independent named matches. Private persistent initialization keeps tile order reproducible across restart. The real-process test proves scoped racks, match isolation, SIGTERM recovery and reconnect sequence fencing; a complete playable MVP still requires the remaining issues.

[Player invitations](docs/specs/player-invites.md) use the static Pages base path and a public query route. The player route checks current match availability and reports malformed, expired or unavailable games before joining. Production joins consume public `VITE_SCRABBLE_ENDPOINT` and `VITE_SCRABBLE_API` build settings; service deployment remains #31.
