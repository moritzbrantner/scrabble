# Scrabble

A shared-board and private-phone multiplayer Scrabble application, under construction. The current slice supplies the browser entry point and Rust workspace; gameplay and network serving are not available yet.

## Development

Install Bun 1.4.2 and Rust (the toolchain file selects 1.98.1 with Clippy and rustfmt). From a fresh checkout:

```sh
bun install --frozen-lockfile
cargo fetch --locked
bun run check
```

`bun run dev` opens the browser development server; visit its printed URL under `/scrabble/`. `bun run dev:server` runs the server scaffold, prints its current availability, and exits. Issue #10 adds the authoritative transport executable.

`bun run check` checks formatting, TypeScript, lint, Rust tests, browser rendering, and the static production build. `bun run format` applies formatting. Dependency changes use `bun install` and commit `bun.lock`; Rust dependency changes include `Cargo.lock`.

## Hosting

The browser uses `/scrabble/` as its Vite base, including in development. GitHub Actions validates and builds once, uploads `dist`, and deploys that artifact on main to GitHub Pages. Configure repository Pages to use GitHub Actions. No server credential belongs in this public static bundle. Production authoritative hosting is tracked in #31.

`cargo` owns canonical game logic under `crates/scrabble-game`; `crates/scrabble-server` will adapt it to `game-server`. `apps/web` owns presentation only. See the ordered GitHub issues #1–#34 for the MVP acceptance requirements.

The versioned domain contracts and offline English fixture ruleset are defined in `crates/scrabble-game`; see [protocol v1](docs/specs/protocol-v1.md) and the [domain glossary](CONTEXT.md). The fixture dictionary is test-only, not a full playable word list.

`GameState` provides deterministic tile storage, initial dealing, replacement draws, and scoped projections. [State storage](docs/specs/domain-state.md) documents the seeded initialization contract and the validation still required before gameplay commits.

`GameSession::apply` now provides the [authoritative turn boundary](docs/specs/turn-application.md) for start, preview/cancel, and pass, with identity, sequence and turn checks. Commit and exchange remain unavailable until their ordered rule/transaction slices.

[Placement validation](docs/specs/placement-validation.md) derives the main and cross words through a pure borrowed-state seam, with structured structural errors. Dictionary acceptance and scoring remain separate checks.

[Scoring](docs/specs/scoring.md) calculates all validated words with new-tile premiums and a single full-rack bonus. The scored storage commit applies cumulative points atomically; browser command integration remains in #19.
