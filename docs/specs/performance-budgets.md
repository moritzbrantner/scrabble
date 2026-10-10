# Scrabble interaction performance contracts

Scrabble measures real gameplay and UI components using its existing Rust and
Playwright test runners. Every measured operation has a **50 ms p95 target**.
Correctness assertions always run; historical ratchets do not redefine this target.

## Measured operations and fixture boundaries

- **React board refresh:** re-render `SharedBoard` with a different public
  snapshot, including React reconciliation, DOM commit and layout of 225 squares.
- **Draft placement:** select a rack tile and place it in `MoveEditor`, including
  both React updates, DOM commit and layout.
- **Draft reset:** clear a two-letter tentative word, including DOM update/layout.
- **Frame-opportunity variants:** start before the same synthetic click dispatch
  (or snapshot state change), perform the same commit/layout work and wait for a
  **second `requestAnimationFrame` callback**. This covers at least one browser
  rendering opportunity after the React commit, but does **not** establish that
  pixels reached the display. It is not hardware input-to-photon latency.
- **Authoritative word commit:** in-process `GameSession::apply`, including move
  validation, dictionary lookup, scoring, replacement draws and atomic state update.
- **Public snapshot:** in-process `GameSession::public_snapshot` after the commit.

Browser measurements use an **optimized Vite `--mode test` build** served by
`vite preview`, not the Vite development server. Only this test build includes
`browser/performance.html` and its test-only fixture entry; ordinary production
releases do not. The fixture renders the same production `SharedBoard` and
`MoveEditor` components.

### Browser workload matrix

| Workload | Shared board | Move editor | Samples |
| --- | --- | --- | --- |
| Opening baseline | Existing playing/finished snapshots with two committed letters | Empty board, authored A and H rack, center squares 112 and 113 | 20 refresh; 24 tile placements; 12 resets |
| Populated desktop | Connected, synthetic 81-tile board alternated with an 82-tile update | Same private rack, with 81 committed tiles; place A and H next to an existing run at squares 118 and 119 | 20 refresh; 24 tile placements; 12 resets, each also measured to second rAF |
| Populated touch emulation | — | Same populated board, Chromium `390 × 844`, touch enabled and mobile viewport | 24 tile placements; 12 resets, each also measured to second rAF |

The populated fixture uses a deterministic connected pattern and the authored
ruleset's available tile inventory, reserving the visible private rack and the
opponent's reported rack count. It exercises large-board reconciliation and
a draft connected to committed tiles. **It is not an authoritative legal
late-game replay**: tile arrangement, scores and turn metadata are synthetic.
The fixture asserts all 225 squares, populated tile counts, tentative tiles,
cleared drafts and committed-tile preservation. Broader blank assignment,
keyboard, accessibility, authoritative snapshot reconciliation and
multi-device correctness remain covered by existing browser tests.

Playwright resolves controls **before** each edit timer. Synchronous measurements
include real React event handling/reconciliation and a forced layout read,
but exclude selector lookups, semantic assertions, initial mount, navigation and
the network. Frame samples share the same start time; awaiting the second rAF
does not move the work out of the measured interval. These are synthetic DOM
click events, not trusted touch/pointer hardware events.

The Rust fixture uses a deterministic 50,000-entry dictionary, four players
and eight accepted pass turns before committing a legal `AT` opening. Fixture
construction, seed search and history setup are excluded from the timed sample.
This tests larger dictionary/history dimensions, not a full-board late-game
native commit. Semantic results are verified independently.

## Repeatable measurements and evidence

From a clean checkout use the README's pinned Bun/Rust dependencies and
Playwright-installed Chromium. Start with the usual advisory performance run:

```sh
cargo test -p scrabble-game --locked --test performance_budget -- --nocapture
bun scripts/test-browser.ts --pages performance-budgets.spec.ts
```

Each browser test prints a `SCRABBLE_PERF_EVIDENCE` JSON record and attaches
`scrabble-performance-evidence` JSON to its Playwright test results. It preserves
raw ordered samples, count, min, p50, p95, p99 and max; the exact Git revision;
Vite build mode; Chromium version/user agent; viewport, DPR, touch capability,
OS/kernel, CPU model/count, memory and an optional target identifier.

For **hard latency acceptance**, use the same named controlled reference
machine, OS, browser, viewport, power mode and refresh-rate conditions on repeated
runs. As a reproducible desktop protocol, use Chromium headless at
`1280 × 900` with the repository's Playwright browser installation and
`--pages` build; keep CPU frequency/power policy and competing system load
consistent and record anything not captured automatically. Set a stable
`SCRABBLE_PERF_TARGET` label identifying that physical runner:

```sh
SCRABBLE_PERF_ENFORCE=1 SCRABBLE_PERF_TARGET=desktop-reference \
  bun scripts/test-browser.ts --pages performance-budgets.spec.ts
```

The optional label describes the controlled target; it does not establish
control by itself. Run the full command repeatedly (for example five independent
sessions). Compare only identical workload/build, browser and machine
fingerprints. Each run enforces the **same unchanged 50 ms p95 limit**, including
populated frame-opportunity variants. A failed strict run is a failure, not a
value to discard or retry until green. The default `bun run check` includes
the browser and native test suites, with semantic correctness blocking.

Shared GitHub `ubuntu-latest` runners cannot serve as a stable reference device:
their latency reports are **advisory** even when below 50 ms; do not enroll
them as a controlled baseline or infer a pass for real devices. If a controlled
runner is unavailable, report latency acceptance as **unverified**.

## Evidence and limitations

Merged [PR #78](https://github.com/moritzbrantner/scrabble/pull/78) recorded these
**shared-runner, optimized-build, opening-workload** p95 values in
[Actions run 37924771444](https://github.com/moritzbrantner/scrabble/actions/runs/37924771444):

| Browser operation | p95 |
| --- | ---: |
| Word-draft placement | 12.30 ms |
| Draft cancellation/reset | 6.60 ms |
| Shared board refresh (225 cells) | 5.30 ms |

Earlier 43–67 ms results came from the development build and included
test-only selector/assertion overhead. They are not evidence of a
production-performance defect. Do not restructure the board solely from
a source-level suspicion about 225 regenerated element descriptions.

A 50 ms interaction budget is not a guarantee of 60 FPS (about 16.67 ms
per frame). Neither layout nor two-rAF browser evidence verifies actual
physical screen paint, native mobile hardware, display compositor timing,
real finger-to-glass latency, late-game authoritative game commits or the
complete networked interaction on two phones. `playable-loop.spec.ts`
independently checks authoritative multi-device behavior, not a latency SLO.
Whole-game latency needs a separately declared device, network path and
measurement boundary. An unmeasured scenario is **not** an accepted pass.
