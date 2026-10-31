# Story end-to-end tests from small state machines

Pilot for checking that every user story works in a real browser against the real Rust server. Each user-story area has one small [XState](https://stately.ai/docs) model. `xstate/graph` generates paths from it, and each path runs as a tagged Playwright test. The result is listed in [story-coverage.md](story-coverage.md).

The models state intended behavior and cite the documents they come from. They import nothing from the application. As with [Decision Atlas 6.3](../DECISION-ATLAS.md), a discovered difference may suggest a model change but never silently rewrites one. A path that hits a real bug becomes `test.fixme` with its issue link (`knownBugs` in a driver), and the model keeps the intended behavior. An unclear intent stays out of the model and goes to the owner.

## Layout

- `apps/web/browser/stories/stories.ts`: the models, their source citations and the registry.
- `apps/web/browser/stories/story-graph.ts`: path generation, with no browser imports.
- `apps/web/browser/stories/story-test.ts`: runs one story's paths. A driver gives one real action per event and one assertion per state.
- `apps/web/browser/stories/*.spec.ts`: the drivers. `match-table.ts` is the board and two phones.
- `apps/web/browser/stories/route-inventory.ts`: every route and query parameter, including unreached ones.
- `scripts/story-coverage.ts` and `.test.ts`: the listing, and the tests that keep it and the inventory honest.

## Running

```sh
bun run test:stories --workers=2                    # the PR paths
bun run test:stories --workers=2 --grep @story:TURN-1
SCRABBLE_STORY_TIER=full bun run test:stories       # adds every simple path
bun run stories:coverage > docs/specs/story-coverage.md
```

The specs live in the development browser suite, so `validate.yml` already runs the PR paths in its shards and `merge-ready` covers them.

## Writing down what worked

Findings from this pilot, for the next repository.

**Check the API in the installed package.** With XState 5.33 the generators are `getShortestPaths`, `getSimplePaths` and `getAdjacencyMap` from `xstate/graph`; the separate `@xstate/test` package is not needed. A model must be pure, with no actors, because traversal calls its transition function repeatedly.

**Filter events with `snapshot.can(event)`.** Without `filterEvents`, traversal treats every unhandled event as a transition to the same state. One 9-state model produced 92 paths instead of 6.

**Shortest paths alone are not enough.** They reach each state once, so they never take a self-loop. The reloads and rejected actions are exactly those. `generatePaths` therefore adds one step for each transition its shortest paths miss.

**Drop prefix paths.** Every step is asserted, so a path that is a prefix of another adds only browser time.

**Put history in the context to model navigation.** Back's target depends on how you got there, so the context holds a bounded `{ stack, index, reloaded }` and back and forward are guarded transitions. `reloaded` makes "navigate, reload, back" a distinct vertex from "navigate, back". Without it the generator never produces that sequence.

**Bound the graph on purpose.** Two history entries, one reload per visit and flat `OPEN_<route>` events from one start give about two paths per route. `MAX_PATHS` fails the run if a model change exceeds its budget. Keep events without payloads, so a path name is a stable test title.

**A fuller tier adds little to models this small.** The `full` tier added only 6 paths, all in ROUTE-NAV. A nightly job for them is not worth its infrastructure here. A larger model may differ, so compare the two counts before adding one.

**Reuse the repository's real seams.** Here those are its per-worker native server, wire observer and native opening selection, not mocks. Setup that a path does not test, such as seating players in TURN-1, belongs in the driver's `open`. That setup is modeled in its own story.

**Assert in every state, not only at the end.** The state assertions are where the value is, and a failure names the exact transition. Use web-first assertions and `expect.poll`; no path uses a sleep.

**Report judgement calls instead of encoding them.** Fallbacks the documents do not specify stay out of the assertions or are marked as such in the model, and an audit list goes to the owner.

## Limits

Coverage describes the declared models, not every possible behavior ([Decision Atlas 6.2](../DECISION-ATLAS.md)). The opt-in explorer in `apps/web/review/state-graph` has its own contract for a longer multi-window journey, and it stays out of required checks. These stories are small enough for a PR.
