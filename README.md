# Scrabble

A shared-board and private-phone multiplayer Scrabble application, under construction. The development board can create authoritative games, publish copyable player links and locally rendered QR codes, and display their live public state. The Rust executable hosts independent matches with runtime health, readiness and graceful recovery. Production endpoint configuration and the remaining gameplay/player workflows are still tracked in the ordered issues.

## Development

Install the latest stable Bun and Rust (the toolchain file selects the stable channel with Clippy and rustfmt). From a fresh checkout:

```sh
bun install --frozen-lockfile
cargo fetch --locked
bunx playwright install --with-deps chromium
bun run check
```

An optional locked Nix shell (`nix develop`, from `flake.nix`/`flake.lock`) provides Bun 1.4.3, Git and rustup. rustup still takes the Rust toolchain channel from `rust-toolchain.toml`, and Playwright's Chromium is still installed with the command above. A second optional environment, `.devcontainer/Dockerfile`, is a digest-pinned Playwright 1.63.0 image (Chromium included). It reads Bun from `package.json` and Rust from `rust-toolchain.toml`, so it adds no second version authority.

Required CI uses neither. The manual `Scrabble Provisioning Comparison` workflow (#81) runs the identical `bun run check` under conventional, Nix and container provisioning, with `cache_state` set to `cold` or `warm`. On 2026-10-17, five samples per variant (ubuntu-24.04, source `5084d2f`) gave these mean job times:

| Variant      |  Cold |  Warm |
| ------------ | ----: | ----: |
| Conventional | 936 s | 854 s |
| Nix          | 990 s | 847 s |
| Container    | 839 s | 858 s |

From checkout to the first focused test, cold runs took 44 s, 64 s and 63 s respectively; warm runs took 51 s, 86 s and 61 s. The full gate (about 620–980 s, noisy) dominates every variant, so none is materially faster. **Recommendation:** keep conventional provisioning for required CI. Keep the Nix shell and the container only as optional reproducible environments for agents. Building the container image once takes about 8 minutes, and pulling it costs about 45 s on every fresh runner. To roll back, delete `flake.nix`, `flake.lock`, `.devcontainer/`, `.github/actions/provision-sample/` and that workflow; nothing else depends on them.

For a playable local game, prepare the English dictionary once, then run `bun run dev:local` and open the complete game URL it prints. This starts the canonical local Compose server and the browser client together. See [local game setup](docs/specs/server-executable.md#local-game-setup) for prerequisites and persistent storage. To host friends on other networks, forward the documented ports and set `LOCAL_PUBLIC_HOST`; see [hosting players on other networks](docs/specs/server-executable.md#hosting-players-on-other-networks).

`bun run dev` opens the browser development server alone. `bun run dev:tls` and `bun run dev:server` remain available for native development. See [server setup and configuration](docs/specs/server-executable.md) for local ports, production requirements, and graceful restart.

`bun run check` checks formatting, TypeScript, lint, Rust tests, browser rendering, the static production build, and Chromium layout and real loopback WebTransport checks. Ready-PR validation runs that full gate; Pages deployment instead validates the public endpoint configuration and builds the production bundle without rerunning the test suite. `bun run format` applies formatting. Dependency changes use `bun install` and commit `bun.lock`; Rust dependency changes include `Cargo.lock`.

The full browser gate runs hex journeys in a separate fixture lifetime capped at four minutes, so graceful shutdown verifies their canonical replay before completed matches retire. Focused arguments to `scripts/test-browser.ts` retain their normal Playwright selection. Failed runs retain private fixture inputs at the path printed in the error output; keep those files private because they include recovery capabilities.

Review scenarios are opt-in and never part of `bun run check`. `bun run review two-player-turn --interactive` starts the same native fixture server, a disposable match, the shared board and separate Ada and Lin browsers. Real UI actions bring it to Ada's turn with a legitimate proposed `AT` placement. The headed browsers then pause in the Playwright Inspector so you can take over: move tiles, commit, or prepare Lin's private pre-move. Resume or close the Inspector to finish and clean up. Rerunning the command replays the same scenario and configuration in a fresh fixture server and match. Without `--interactive`, `bun run review` runs the same setup headless with a smoke assertion. Screenshots of each view go to `test-results/review/`. Add `--trace on` for a Playwright trace.

`bun run review state-graph` explores the independent expected-state contract in `apps/web/review/state-graph/contract.ts` (ADR 0012). It plans journeys that cover every declared transition, including the rejected invite, the rejected `TA` commit and Ada's disconnect and reconnect. Each journey replays real UI actions in a fresh disposable match, and every reached state is checked against the board's and Ada's wire snapshots and the visible UI. The run writes `state-graph-report.md` and `.json` to `test-results/review/` with per-state and per-transition passed/failed/untested results, the reproducible action path for each edge, and screenshots of each view after every step. Coverage describes the declared contract, not complete application correctness. The interactive graph, live state following and control discovery are still to come.

Match setup starts untimed. Enable clocks explicitly to choose cumulative per-player
or fresh per-turn time, exact durations, and automatic pass or forfeit on expiry.
Cumulative automatic pass requires a positive turn-start increment. The server
owns expiration; board and controller display the selected rules and remaining
time. A forfeiter keeps a frozen score/rack below remaining players in the standings,
and the last remaining player wins. No manual pause is available. See the
[clock contract](docs/specs/match-clock.md) and [presentation boundary](docs/adr/0011-match-clock-presentation-and-forfeits.md).

## Hosting

The GitHub Pages client connects to the authoritative Scrabble service on the Hetzner server at `moenarch.com`. `multiplayer-setup-service` is a signaling service and is not required by this transport. Local hosting remains available.

The [moenarch.com launch link boundary](docs/specs/moenarch-launch.md) supports an explicit host-board popup and canonical player links. The setup service binds only public match references; private board/player capabilities remain in Scrabble. Publication and independent live launcher verification are tracked in #80.

The browser uses `/scrabble/` as its Vite base, including in development. GitHub Actions validates and builds once, uploads `dist`, and deploys that artifact on main to GitHub Pages. Configure repository Pages to use GitHub Actions. No server credential belongs in this public static bundle. See [production deployment](docs/specs/production-deployment.md) for the deployed public topology and rollout procedure. Public endpoint settings live in `deploy/client.json`.

`cargo` owns canonical game logic under `crates/scrabble-game`; `crates/scrabble-server` adapts it to `game-server` through `ScrabbleSimulation`. `apps/web` owns presentation only. See the ordered GitHub issues #1–#34 for the MVP acceptance requirements.

The versioned domain contracts and offline English fixture ruleset are defined in `crates/scrabble-game`; see [protocol v1](docs/specs/protocol-v1.md) and the [domain glossary](CONTEXT.md). The default fixture dictionary is test-only; deployments can load a separate local list through the [dictionary boundary](docs/specs/dictionary-validation.md).

`GameState` provides deterministic tile storage, initial dealing, replacement draws, and scoped projections. [State storage](docs/specs/domain-state.md) documents the seeded initialization contract and the validation still required before gameplay commits.

`GameSession::apply` now provides the [authoritative turn boundary](docs/specs/turn-application.md) for start, preview/cancel, commit, pass and exchange, with identity, sequence and turn checks. [Game completion](docs/specs/game-completion.md) applies final rack deductions, go-out transfers, and explicit ties, then freezes gameplay. [Deterministic history](docs/specs/deterministic-history.md) exposes accepted public turns and verifies private native replay evidence offline. [Mobile interaction and accessibility](docs/specs/mobile-accessibility.md) covers touch layouts, keyboard gameplay, themes, and interface languages. [Configurable rulesets](docs/specs/configurable-rulesets.md) bind deployment-owned alphabets, tile distributions, and scoring settings to immutable replay evidence.

[Placement validation](docs/specs/placement-validation.md) derives the main and cross words through a pure borrowed-state seam, with structured structural errors. Dictionary acceptance and scoring remain separate checks.

[Scoring](docs/specs/scoring.md) calculates all validated words with new-tile premiums and a single full-rack bonus. The scored storage commit applies cumulative points atomically; browser command integration remains in #19.

The [shared board](docs/specs/shared-board.md) renders public snapshot fixtures for lobby, playing, and finished phases. Add `?fixture=playing` or `?fixture=finished` to the `/scrabble/` entry point to inspect those presentation cases. Fixture previews are development-only. A production build requires the public service configuration or displays an explicit service-unavailable message. `bun run test:browser` runs focused layout checks and writes screenshots to ignored `test-results/`.

The [browser transport](docs/specs/browser-transport.md) supports versioned welcome, bounded commands, latest-state snapshot reassembly, reliable control and in-memory reconnect capabilities. Its real-network fixture exercises upstream `game-server` independently of Scrabble rules. Public deployment is tracked in #67.

The [simulation adapter](docs/specs/simulation-adapter.md) maps runtime admissions directly to domain identities, applies authenticated commands, keeps ticks independent of turns, and separates canonical replay evidence from private player projections. Browser-free tests verify runtime replay, recovery, reconnect fencing and grace expiry. [Private client boundaries](docs/specs/private-client-boundaries.md) document scoped projections, rejected malicious commands, capability fencing, and redacted diagnostics.

Guests can import, save, share and copy [custom word lists](docs/specs/word-list-library.md) without an account; saved lists are selected independently of tiles and board, and started matches keep their pinned word set.

The [creation API](docs/specs/game-creation.md) derives high-entropy public match IDs, bounds retries and match lifetime, rejects draining/full hosts, and retires through the upstream fencing boundary. Chromium acceptance drops a successful response, retries to recover the same game, and joins its canonical route.

The [authoritative executable](docs/specs/server-executable.md) reuses game-server hosting/status/recovery for independent named matches. Private persistent initialization keeps tile order reproducible across restart. The real-process test proves scoped racks, match isolation, SIGTERM recovery and reconnect sequence fencing; a complete playable MVP still requires the remaining issues.

[Player invitations](docs/specs/player-invites.md) use the static Pages base path and a public query route. The player route checks current match availability and reports malformed, expired or unavailable games before joining. Production joins consume public `VITE_SCRABBLE_ENDPOINT` and `VITE_SCRABBLE_API` build settings; public deployment is recorded in #67.
