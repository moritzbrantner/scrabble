# Independent UI-state verification and browser exploration

Status: accepted technical direction from owner `/steer` on 2026-10-21 (Q1–Q10). The contract, generated traversal and coverage report are implemented in `apps/web/review/state-graph/` (`bun run review state-graph`); the interactive graph, live observation and control discovery are not yet implemented. Product outcome: [Decision Atlas 6.1–6.3](../DECISION-ATLAS.md#6-developer-review-and-model-based-state-exploration).

## Ownership

An explicit, reviewed, version-controlled state/transition contract belongs to the **development verification tooling**, not to React routing or Rust game logic. It is the independent expected-behavior oracle for the explorer and model-derived browser journeys. Do not derive the expected graph by importing the application's route/transition implementation; doing so would make implementation defects self-validating. Do not insert an application-wide runtime state machine solely to support tests.

The existing Rust match/session/protocol implementations retain gameplay authority and privacy. Observable browser behavior and authorized server responses establish the _observed_ state. The explorer may match those observations to expected graph states; it must report ambiguity and unknown states instead of inventing an expected transition.

## Execution

Reuse Scrabble's existing Playwright/browser fixtures and the interactive local review launch established by [#185](https://github.com/moritzbrantner/scrabble/issues/185). A directed graph transition drives an actual user action and verifies its observable result. Independent manual actions in the board or two player contexts resynchronize the highlighted state. Captured actionable controls lacking a declared transition are review candidates, with explicit exclusions for intentional non-modeled controls; discovery never automatically changes the expected model.

To select a historical state, start a **fresh, disposable authoritative match**, replay a deterministic sequence of real actions, and verify the resulting state. Do not rewind the live match, inject private/game state into production clients, or use saved checkpoint restoration in this initial approach. Isolate sessions and clean up test resources; never expose private player data in another client's view.

The complete modeled multiplayer journey and rejection/reconnection cases are explored **on demand**. Report verified, failing and untested modeled states and transitions with screenshots/traces where useful. Treat graph coverage as coverage of a finite declared contract, not proof of exhaustive application behavior. The normal required CI gate is unchanged unless separately approved.

## Reuse and alternatives

A runtime XState/Statig machine is not mandated. Use existing router, Rust ownership and Playwright first; a graph library may be selected by the implementing worker if it materially simplifies traversal or visualization. Rejected initial alternatives: production UI driven by this verification model, an embedded public-game developer screen, restored-state checkpoint jumping, and automatic acceptance of inferred model changes. Avoid a new shared framework, server or dashboard that duplicates [#185](https://github.com/moritzbrantner/scrabble/issues/185)'s session foundations.
