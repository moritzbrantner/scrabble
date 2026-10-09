# Scrabble interaction performance contracts (initial experiment)

Scrabble measures real gameplay and UI components using its existing Rust and
Playwright test runners. Every measured operation has a **50 ms p95 target**.
Correctness assertions always run; historical ratchets do not redefine this target.

## Measured operations

- **React board refresh:** re-render `SharedBoard` with a different authored public
  snapshot, including React reconciliation, DOM commit and layout of 225 squares.
- **Draft placement:** select a rack tile and place it in `MoveEditor`, including
  the two React updates, DOM commit and layout.
- **Draft reset:** clear a two-letter tentative word, including DOM update/layout.
- **Authoritative word commit:** in-process `GameSession::apply`, including move
  validation, dictionary lookup, scoring, replacement draws and atomic state update.
- **Public snapshot:** in-process `GameSession::public_snapshot` after the commit.

The browser measurements execute an **optimized Vite test build**, not the Vite
development server. Only that test build includes
`browser/performance.html` and its test-only fixture entry; the ordinary
production release does not include it. The fixture uses the same production
`SharedBoard` and `MoveEditor` components.

The Rust fixture uses a deterministic 50,000-entry dictionary, four players and
eight accepted pass turns before committing a legal `AT` opening. Fixture
construction, seed search and history setup are excluded from the timed sample.
This tests larger dictionary and history dimensions, but it is not a full-board
late-game workload. The operation's semantic results are verified independently.

Playwright locates its controls **before** starting each edit timer. The interval
includes real React event handling/reconciliation and a layout read, but excludes
test-only selection and correctness assertions. Browser `performance.now()` and
Rust `Instant` provide the source measurements; navigation, network
acknowledgement, initial loading and actual screen paint are out of scope.

## Run

From the repository root, after the README's documented setup:

```sh
cargo test -p scrabble-game --locked --test performance_budget -- --nocapture
bun scripts/test-browser.ts --pages performance-budgets.spec.ts
```

The default `bun run check` includes both test suites. They report measured
p95 values and **always block incorrect results**, but do not reject shared
GitHub runners solely for variable wall-clock samples. A controlled target
runner enables the same unchanged 50 ms latency ceilings with:

```sh
SCRABBLE_PERF_ENFORCE=1 cargo test -p scrabble-game --locked --test performance_budget -- --nocapture
SCRABBLE_PERF_ENFORCE=1 bun scripts/test-browser.ts --pages performance-budgets.spec.ts
```

Shared CI host descheduling can exceed 50 ms even for unchanged programs; hard
latency acceptance therefore requires a named, sufficiently repeatable runner.
Unavailable or incomparable target-device latency evidence is not a pass.
The 50 ms threshold is not fitted to benchmarks or silently raised. A small
historical change from 4 to 5 ms is acceptable if the declared target passes.

## What this does not prove

A 50 ms response target is distinct from 60 FPS (approximately 16.67 ms per
frame). These tests do not establish paint timing, interaction-to-network-result
latency on two phones, full-board late-game capacity or cross-device performance.
`playable-loop.spec.ts` independently checks authoritative multi-device behavior;
future whole-game latency tests must specify the actual device/network profile
and measure the entire declared interaction boundary.
