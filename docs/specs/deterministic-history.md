# Deterministic turn history and offline verification

Every accepted commit, pass, and exchange publishes one `PublicTurn` in the same candidate state as board, rack, bag, scores, and turn completion. There is no browser-maintained history, separate event log, or second gameplay store. Previews, connection/presence changes, rejected commands, and stale retries add no turn. Native `game-server` replay remains the authority: replaying its accepted commands regenerates this projection and verifies the same canonical checkpoints and final hash.

A turn contains its zero-based absolute turn ID, authenticated player ID, and a tagged action. Commits include structurally validated main/cross words in stable board order, the awarded move score, and the number of newly played blanks. Passes contain only their kind. Exchanges contain only a tile count, never the exchanged letters or tile identities. Every entry contains resulting scores and signed deltas for all seated players. The final entry includes rack deductions and go-out transfers; those adjustments cannot be mistaken for the commit's move score. Delta encoding is i64 because the difference of two i32 scores can exceed i32.

`GameState` retains the latest **24** turns, including them in explicit private canonical evidence and every public/player projection. The shared board renders the same snapshot history in lobby/playing/finished presentation and indicates when earlier turns were omitted. Reconnect and refresh obtain it with the ordinary authoritative snapshot. Retained turn IDs do not restart at zero. A maximum-size UTF-8 projection test proves the bound fits the native 65,535-byte snapshot payload, including the full board, four players, and a private rack. The complete accepted command sequence remains in private native replay/recovery evidence; the public ring is deliberately bounded.

## Offline path

With the original server initialization settings (persisted seed path, rules/dictionary identity and file, and configured environment), run:

```bash
cargo run -p scrabble-server --example verify_replay --locked -- MATCH_ID /absolute/path/to/match-recovery-image
```

Use `<MATCH_ID>.recovery` in the upstream-owned recovery bundle described in [server recovery](server-executable.md). Operate on a retained copy before startup consumes that bundle. `RecoveryImage::read_file` bounds and validates the input; `MatchFactory` reconstructs the original deterministic initialization; native `verify_replay` verifies authoritative records/checkpoints. A mismatched match, initialization, dictionary, or canonical version fails verification. The command prints JSON with verified record/checkpoint counts, final tick, hexadecimal canonical hash, and only public game ID, phase and bounded history. It does not start a listener or consume/write the recovery bundle. No racks, bag, seeds, raw command payloads, board proof, or capabilities enter its report.

Full private evidence follows the existing six-hour lifetime and graceful recovery contract. Continuous crash persistence, archival retention after native retirement, and a public full-history export are separate deployment choices; this change does not introduce them. Native compact digest checkpoints retain their existing recovery budget.

## Compatibility and evidence

Private game evidence is version **3**. Versions 1/2 fail the initial checkpoint instead of being silently interpreted with added history; keep the matching old executable and initialization to inspect old bundles. Outer simulation, commands, and native codecs are unchanged. The browser/server public DTO now requires `history` and must be deployed together; strict readers expose an incompatible snapshot rather than guessing a timeline.

Runtime tests cover blank and normal commits, rejected commit/preview behavior, exchange privacy, pass, final adjustments, reconnect rotation, full replay/hash equality, recovery equality, and a 33-turn game whose public ring retains turns 9–32. Offline inspection tests read a real persisted native recovery image, verify its hash, reject a different match, and check the report's exact public keys. The built Pages three-client test independently checks played words/scores, pass/exchange entries, turn order, finished convergence, and unchanged history after phone and board refresh. The public fixtures remain Rust-generated. An executable smoke check also verified a saved six-pass finished image through the offline command, matching the live runtime hash and emitting only public report fields.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
