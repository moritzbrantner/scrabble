# Initial Scrabble interaction performance budgets

These are executable **playability smoke contracts**, alongside (never replacing) the
existing semantic, accessibility and multiplayer correctness tests. They measure
real Scrabble code rather than setting a 50 ms Playwright timeout.

| Operation | Measured boundary | p95 ceiling | Test |
| --- | --- | --- | --- |
| Read an updated shared board | Warm React `SharedBoard` refresh, DOM commit and forced layout of 225 squares | 50 ms | `performance-budgets.spec.ts` |
| Compose a word | Select one rack letter and place it on the 225-square `MoveEditor` board, including DOM update and layout | 50 ms per placement | `performance-budgets.spec.ts` |
| Cancel a draft | Cancel a two-letter tentative word and restore the board | 50 ms | `performance-budgets.spec.ts` |
| Accept a valid word | In-process `GameSession::apply` for a real dictionary-accepted `AT` opening, including score, draw, and state commit | 50 ms | `performance_budget.rs` |
| Read canonical public board state | In-process `GameSession::public_snapshot` after that commit | 50 ms | `performance_budget.rs` |

The browser tests exercise production `SharedBoard` and `MoveEditor` components
through a test-only fixture using `flushSync` to establish the **synchronous
interaction-to-visible-DOM boundary**. They exclude page navigation, initial
mount, actual paint and network transport. The native test excludes game/dictionary
construction; its commit timing includes full authoritative validation/commit.
Correctness checks ensure all 225 visible cells and the two placed draft letters
remain present, and that a committed word produces the expected turn, score and
board content. These are not merely timing a no-op.

Both tests collect repeated samples, sort them and require p95 <= 50 ms. Every
sample is measured with the clock in the executing runtime (browser
`performance.now()` or Rust `Instant`). Timing does **not** include Playwright
driver waits, locator discovery outside the browser, or construction of native
fixtures. These budgets are not a historical speedup ratchet: 4 -> 5 ms passes if
the declared 50 ms contract still passes.

## Run

From the repository root, after its documented toolchain/setup:

```sh
cargo test -p scrabble-game --locked --test performance_budget -- --nocapture
bun scripts/test-browser.ts performance-budgets.spec.ts
```

Both suites also run under the existing `bun run check` / browser test
workflow. They do not require a new benchmarking framework.

## Limits of the guarantee

These checks detect gross UI/main-thread and native processing regressions on
the execution host. GitHub-hosted runners are shared and their wall-clock timing
varies: a hard 50 ms test there is a **provisional smoke gate**, not a
statistical guarantee across target phones or browsers. If noise becomes a
problem, move hard latency enforcement to a controlled reference device/runner;
do not silently increase the budget to make CI green. Preserve deterministic
correctness in normal CI and report incomparable or unavailable performance
evidence honestly.

A 50 ms user-action budget is **not** a 60 FPS frame budget (16.67 ms total
frame interval). These tests also do not guarantee server/network
acknowledgement, full two-phone word-play responsiveness, loading, animation,
or GPU paint latency. Existing `playable-loop.spec.ts` covers authoritative
end-to-end behavior, but its round-trip time is not constrained to 50 ms.
A real-device, full-turn/frame-pacing budget must be a separately specified
and measured contract with a named hardware tier and network conditions.
