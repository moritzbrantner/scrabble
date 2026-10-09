# Initial Scrabble interaction performance budgets

These are executable **playability smoke contracts**, alongside (never replacing) the
existing semantic, accessibility and multiplayer correctness tests. They measure
real Scrabble code rather than setting a 50 ms Playwright timeout.

| Operation                         | Measured boundary                                                                                                    | p95 ceiling         | Test                          |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ------------------- | ----------------------------- |
| Read a rendered public board      | Inspect 225 accessible DOM squares of the mounted `SharedBoard` and force layout                                                         | 50 ms               | `performance-budgets.spec.ts` |
| Compose a word                    | Select one rack letter and place it on the 225-square `MoveEditor` board, including DOM update and layout            | 50 ms per placement | `performance-budgets.spec.ts` |
| Cancel a draft                    | Cancel a two-letter tentative word and restore the board                                                             | 50 ms               | `performance-budgets.spec.ts` |
| Accept a valid word               | In-process `GameSession::apply` for a real dictionary-accepted `AT` opening, including score, draw, and state commit | 50 ms               | `performance_budget.rs`       |
| Read canonical public board state | In-process `GameSession::public_snapshot` after that commit                                                          | 50 ms               | `performance_budget.rs`       |

The browser tests inspect the real mounted `SharedBoard` DOM and render the actual
`MoveEditor` in a test-only fixture. The edit path uses `flushSync` to establish
its **synchronous interaction-to-visible-DOM boundary**. The board-read path
is not a React-refresh benchmark. Both exclude page navigation, initial mount,
actual paint and network transport. The native test excludes game/dictionary
construction; its commit timing includes full authoritative validation/commit.
Correctness checks ensure all 225 visible cells and the two placed draft letters
remain present, and that a committed word produces the expected turn, score and
board content. These are not merely timing a no-op.

Both test suites collect repeated samples and retain an unchanged p95 <= 50 ms target. Every
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

The native test enforces 50 ms in normal CI. The browser tests run and report
observed p95 measurements under the existing `bun run check` workflow, but hard
browser timing enforcement requires an explicitly configured controlled runner:

```sh
SCRABBLE_PERF_ENFORCE=1 bun scripts/test-browser.ts performance-budgets.spec.ts
```

The browser assertions still check real board/draft results in regular CI.
None of these tests requires a new benchmark framework.

## Limits of the guarantee

These checks detect gross UI/main-thread and native processing regressions on
the execution host. GitHub-hosted runners are shared and their wall-clock timing
varies: browser p95 on such runners is **advisory**, not an authoritative latency
verdict. A controlled reference device/runner may opt in to a hard 50 ms gate via
`SCRABBLE_PERF_ENFORCE=1`. This does not relax or auto-adjust the 50 ms target.
Do not claim an across-device guarantee without running an appropriate reference
profile. Preserve deterministic
correctness in normal CI and report incomparable or unavailable performance
evidence honestly.

A 50 ms user-action budget is **not** a 60 FPS frame budget (16.67 ms total
frame interval). These tests also do not guarantee server/network
acknowledgement, full two-phone word-play responsiveness, loading, animation,
or GPU paint latency. Existing `playable-loop.spec.ts` covers authoritative
end-to-end behavior, but its round-trip time is not constrained to 50 ms.
A real-device, full-turn/frame-pacing budget must be a separately specified
and measured contract with a named hardware tier and network conditions.
