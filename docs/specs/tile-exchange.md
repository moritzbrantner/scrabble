# Atomic private tile exchange

The active authenticated phone can select one or more rack identities and confirm an exchange. The phone suspends placement editing and clears its public placement preview while choosing private exchange tiles. Cancelling restores the draft and preview; submission pauses preview publication and waits for the authoritative snapshot. No client changes rack ownership optimistically.

The native turn boundary rejects inactive players, stale turns/sequences, empty selections, duplicates, non-owned identities, bags below `exchange_minimum_bag`, and selections larger than the available bag. Failed validation and turn/revision overflow publish no changes and consume no sequence.

Replacements are drawn from the existing bag tail before exchanged tiles return, guaranteeing a distinct replacement for every selected identity. Unselected rack tiles retain their order. Returned tiles are sorted by identity, then the remaining bag is shuffled with the original private seed using ChaCha8 stream `turn + 1`. Stream zero belongs to the initial shuffle; distinct turns use distinct streams. Input selection order therefore cannot change the resulting bag. The immutable seed is already a private replay/factory input and is retained only inside the engine. Existing canonical evidence and pre-exchange replay hashes remain unchanged. A successful exchange advances once and clears the preview, while preserving board, scores, rack sizes and bag size. Public projections contain no exchange selections or bag order. Scoreless completion follows in #25.

Native fixtures cover minimum-bag boundaries, insufficient replacement counts, blank tiles, exact replacement identities, conservation, private projections, duplicate/stale fencing, overflow, multiple exchanges, replay and recovery. The built Pages scenario verifies two-phone selection/cancellation, preview suppression, authoritative convergence, unchanged other rack, and refreshed replacement ownership.

Full gate: `bun run check`. Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
