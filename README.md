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
