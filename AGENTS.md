# Agent guidance

## Domain and authority
- Read `CONTEXT.md` for the match, rack, bag, proposed placement, preview, and committed-move vocabulary.
- Preserve a single authoritative ruleset, turn and score transition, board state, and private rack ownership. Inspect the existing Rust game/server and browser interfaces before changing their boundary.
- `apps/web` owns browser presentation and interaction; do not implement server-authoritative game rules in React.
- Consult `README.md` and the relevant `docs/` decision before changing deployment, lobby/hosting, networking, or performance contracts.

## Shared conventions and task inspection
- Repository-local instructions take precedence over applicable installed convention modules. Until the official installer has populated `conventions.json`, `conventions.lock.json`, and `.conventions/`, resolve the current `coding-agent-conventions` source through `coding-tooling` and report unavailable policy rather than guessing.
- Prefer `coding-tooling inspect --target <existing-path> --json` to find relevant context and checks. Retain root/ancestor instructions and the repository completion gate.

## Verification
- The root `package.json` scripts intentionally invoke both Bun and Cargo. To avoid repeating Rust workspace checks for every discovered subcomponent, use `coding-tooling plan --tier fast --component scrabble --json` and `coding-tooling run --tier fast --component scrabble --strict --json` for the root application.
- Use focused tests while iterating. The complete source-owned gate is `bun run check`, which includes the production build and browser tests. `.coding-tooling.json` also exposes those as the full tier on the root `scrabble` component.
- Hosted validation additionally checks deployment. Missing Playwright browsers or other dependencies are environment failures, not evidence that an application test passed.
- Keep real interaction and performance evidence; never silently relax the existing 50 ms interaction contract.
