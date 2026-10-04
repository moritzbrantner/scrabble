# Game completion and final scoring

An accepted commit, pass, or exchange completes exactly one turn through the native `GameState::complete_turn` boundary. Previews and rejected commands do not count. Passes, exchanges, and commits whose total score is zero increment the consecutive scoreless counter; any positive commit resets it. The configured `scoreless_turn_limit` ends the game when reached (six for the English fixture).

A commit also ends the game when replacement draws leave the bag empty and the active player's rack empty. An empty bag with tiles still in the rack continues play. Going out takes precedence if a zero-point commit meets both end conditions.

At either ending, each player's score loses the sum of the values of their remaining rack tiles. Blanks contribute zero. When a player goes out, that player additionally receives the total rack value deducted from every opponent. A scoreless ending makes deductions without a transfer. Scores may be negative. Every player sharing the highest final score appears in `Phase::Finished.winners`, in stable seat order; ties have no arbitrary winner.

All counter, revision, and score arithmetic is checked before final publication. Commits and exchanges run on a candidate state, so failure also preserves placement, replacement draws, exchange shuffle, and cumulative scoring. A failed pass preserves its counter and scores. Successful completion clears tentative previews and consumes the command sequence once. Finished sessions reject every gameplay command; duplicate commands cannot apply adjustments again.

The existing public snapshot carries only final scores, winner identities, board, bag count, and rack counts. Each phone retains its own private rack. The board displays the winner or tie; phones replace the move editor with their read-only rack and final scores. No rack history or bag order is published. Completed-game summaries and rematches remain issue #33.

## Replay compatibility

Private game canonical evidence is version **2** and includes `consecutive_scoreless_turns`. The outer simulation and browser DTO versions are unchanged. The ruleset's existing scoring data and content identity remain unchanged. Adding completion changes accepted-turn semantics, so version 1 game histories are intentionally not recovered by this engine: their initial checkpoint/hash fails verification before commands are replayed. Keep the prior executable and matching dictionary configuration to replay those histories; this change provides no migration or silent reinterpretation. Version 2 finished games replay and recover with identical canonical state and final adjustments. Connection flags remain live presentation state and correctly start disconnected after recovery.

## Evidence

- Native completion tests cover two through four players going out, rack transfers, negative scores, explicit ties, mixed zero-point commits/exchanges/passes, positive-score reset, simultaneous end conditions, command freezing, duplicate rejection, and tile conservation.
- Internal arithmetic tests reject final score overflow without publishing a board, score, rack, preview, sequence, or counter change.
- Runtime tests verify both endings through real replay and recovery, final lifecycle retention time, scoped privacy, rejection without replay-record mutation, and refusal of version 1 game evidence.
- The native public fixture reaches `Finished` through accepted passes. The built Pages browser scenario completes the real three-client match, independently calculates final rack deductions, checks converged final scores/winners, and refreshes a finished phone with no gameplay controls.

Shared convention source revision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
