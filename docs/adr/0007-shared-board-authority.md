# Shared board authority outside playing seats

Status: accepted

## Decision

New matches distinguish runtime actors from Scrabble players. The shared board is an authenticated runtime actor, but has no playing seat, score, turn, or tiles. Up to four named phone actors become playing seats through authenticated `set_name` commands. An actor awaiting name admission receives the public lobby and an empty recipient-scoped rack. Only named seats participate in minimum player validation and dealing.

The board proves ownership with `claim_board` and the private 128-bit creation request nonce. The match factory verifies the proof using its persisted derivation key and the match's creation timestamp. A public invitation, chosen name, claimed player ID, or admission order cannot confer board authority. A seated player cannot claim the board; a claimed board cannot acquire a seat. The owner ID belongs to canonical simulation state. The immutable verifier belongs to factory construction inputs and is excluded from serialization and Debug.

The runtime still owns admission IDs, reconnect capabilities, connection epochs, sequencing, transport and replay. Its capacity is five actors: one board and four phone players. The adapter does not introduce another session registry. Expiry removes an undealt named seat or pending actor; it clears board ownership when that owner expires. Playing seat expiry continues to preserve rack ownership until #27 defines forfeiture.

Public projections include `host: { id }` for this match mode, with a null ID until a board claims ownership. The board's recipient projection always has an empty `own_rack.tiles`. The public playing roster excludes it. The existing round-robin deal runs once over named seats in authoritative registration order; the first seat becomes active at turn zero. Start from another actor, repeated Start, and new admission after play begins are rejected.

## Version and recovery compatibility

Version-one creation requests continue to derive `g_<timestamp>_<tag>` using the unchanged `scrabble/create/v1` HMAC domain. Their factory construction and version-one canonical bytes retain the existing board-as-player behavior for recovery compatibility. Changing construction rules under those IDs would invalidate previously captured replay checkpoints and could allocate a different game when an old creation response is retried.

The separate-board mode uses `b_<timestamp>_<tag>` under `scrabble/create/v2` and canonical adapter format version two. Factory reconstruction selects the same mode from the ID, including on recovery. The opaque tag remains 128 bits, keeping IDs within the runtime's 64-byte limit. The nonce remains private; neither creation responses nor public projections carry it. Private command replay retains the authenticated ownership claim so recovery can verify it again against the persisted factory input.

## Evidence and integration

Native tests cross the factory, simulation and real runtime interfaces. They prove board proof scope and persistence, rejection atomicity for identity/sequence/turn/name errors, minimum players, four disjoint seven-tile racks, 100-tile conservation, an empty board rack, identical public projections, deterministic first turn, rejected repeated Start and late joins, replay equality and recovery. Restored connection flags are checked separately from canonical game state.

The HTTP endpoint accepts both creation versions; the browser creates version two and submits the private board claim. Pending phone snapshots allow name admission before a seat exists. The board saves only its route-bound reconnect capsule under a separate `scrabble:board:v1:` tab-storage key, allowing refresh to reuse its authority without retaining the creation nonce. Phone capsules remain under `scrabble:player:v1:`. A board link without either the original private creation request or an existing board capsule cannot allocate a replacement owner.

Chromium observes the decoded wire seam while starting a board and four isolated phones. It checks an empty board rack, four disjoint seven-tile phone racks, identical public game/order/active/turn/bag metadata, disabled Start with fewer than two named players, snapshot-confirmed Start and retry, full and closed game errors, and retained board/phone identity across reload. Native process coverage retains a legacy creation beside a version-two creation through restart. Broader reconnect and departure behavior remain #26 and #27.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
