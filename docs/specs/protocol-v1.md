# Scrabble payload contract v1

The Rust types in `crates/scrabble-game/src/protocol.rs` own the payload contract. This slice defines contracts; command authorization, canonical state transitions, and browser decoding arrive in subsequent issues.

## Transport and identity

UTF-8 JSON payloads travel inside `game-server` command/snapshot messages. The payload `version` is the number `1`; unknown versions fail explicitly. Unknown fields and command kinds fail closed. Identifiers are canonical unsigned decimal strings spanning u64: `"0"` through `"18446744073709551615"`. Leading zeroes, signs, numeric JSON identifiers, and out-of-range values are rejected. The transport command sequence is an unsigned u32 JSON number.

A command carries `game_id`, `player_id`, `sequence`, and `expected_turn`. The future simulation adapter compares game/player claims and sequence against transport-authenticated identity and sequence; a payload player ID cannot confer authority. Connection epoch and reconnect capability fencing belong to game-server and are never accepted from this payload. Start requires lobby host authority; gameplay and preview require the active player and current turn. Retire/create/rematch are session-control operations, outside gameplay commands.

Example pass payload:

```json
{
  "version": 1,
  "game_id": "1",
  "player_id": "2",
  "sequence": 12,
  "expected_turn": "3",
  "command": { "kind": "pass" }
}
```

Commands are `start`, `preview`, `commit`, `pass`, and `exchange`. Preview and commit carry `placements`, each containing `tile_id`, `coordinate` (`row`, `column`), and `blank_as` (one Unicode scalar or null). Exchange carries `tile_ids`. Coordinates are zero-based with v1 bounds 0–14; the selected ruleset can impose a smaller board. The decoder limits a command to 16 KiB and a tile list to 15. Ruleset rack limits, duplicate identities/coordinates, rack ownership, alphabet membership, and structural move legality remain authoritative engine validation. Empty preview clears the preview; empty commits and exchanges must fail gameplay validation.

## Snapshots and authority

`PublicSnapshot` contains only version, game identity, revision, ruleset/dictionary identity, phase, committed board tiles, public player names/scores/rack counts, remaining-tile count, and an optional current preview. `Phase` is a discriminated union: lobby; playing with active player and turn; finished with all tied winners. Board tiles reveal letter and blank status, without canonical tile IDs. Players have stable public IDs; their racks are absent from public types.

`PlayerSnapshot` combines that public view with exactly one `own_rack`, tagged with its owning player ID. A server projection must select it using authenticated identity, never a browser-provided lookup parameter. Bag order, other racks, random seed, reconnect capabilities, and canonical recovery bytes have no fields in browser snapshots. Public and player-scoped serialization tests are not substitutes for later end-to-end disclosure tests (#27).

Canonical state comprises committed tiles, player racks, bag/order, scores, phase/turn, selected ruleset/content identity, and deterministic random state. Local selection, drag state, errors, and draft editing are presentation state. Preview is ephemeral: it is cleared on turn changes, commit, disconnect/replacement, and recovery, and excluded from canonical replay hashes. Public snapshot revision is an authoritative state revision, not a render frame or wall clock. Collections in canonical projections must have explicit stable ordering: board by row/column, players by seat, rack by deterministic rack order.

## Initial configuration

`ruleset::english_fixture()` constructs `en-classic-fixture` revision `1`, referencing `scrabble-authored-fixture` revision `1`, without network access. It declares the English 100-tile distribution, a 15×15 standard premium layout, seven-tile racks, 50-point bingo, a seven-tile exchange threshold, six scoreless turns, and two to four players. `Ruleset::validate` rejects incomplete identity, invalid dimensions, duplicate letters/blanks, nonzero blank values, or distributions that cannot supply initial racks.

`crates/scrabble-game/fixtures/words.txt` is a tiny sorted, repository-authored fixture under the project MIT license. It is deliberately unsuitable for ordinary English play. Dictionary lookup and deployment-supplied licensed/open word lists are implemented in #24; production dictionary provenance is a release requirement. Any change to dictionary contents or rules semantics requires a new explicit content revision; recovery and replay must retain both identities and reject mismatches. A running match cannot switch rulesets.

Tests in `crates/scrabble-game/tests/contracts.rs` prove strict version/identifier parsing, all command encodings, bounds, public-field isolation, and offline initial ruleset configuration. The browser transport adapter will consume this versioned shape in #8.
