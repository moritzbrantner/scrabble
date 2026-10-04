# Authoritative replacement draws

A successful commit transfers the selected tile identities to the board, scores the move, refills only the committing rack, and advances the turn inside one atomic candidate. Errors publish none of these changes. Refill uses the existing seeded tail-draw order, stops at configured rack capacity, and takes only the remaining bag tiles. A legal move completes when the bag is empty; game completion and final scores are handled in #25.

Updated rack identities and faces appear only in the owner's scoped snapshot. Other recipients receive their own unchanged rack and the same public board, scores, turn, rack counts and remaining-tile count. Browser rack rendering and editor reconciliation consume the authoritative snapshot; no UI acknowledgement gates the next player's authority.

Native commit cases cover normal refill, one remaining tile and no remaining tiles, exact tail-draw identities, committed identity removal, untouched other racks, and tile conservation. Counter-overflow tests verify rollback even after candidate draws. The real runtime proves duplicate commits cannot draw twice, the next player can act while the committing player is disconnected, reconnect and recovery preserve the replacement rack, and replay reproduces the same canonical state hash. Chromium verifies scoped wire/DOM replacement identities and their preservation after a phone refresh.

Full gate: `bun run check`. Shared policy sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
