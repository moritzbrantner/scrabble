# Main-board lobby

Creating a game replaces the current public URL with its canonical match ID. Reopening that URL joins the same lobby without issuing another creation request. Reload-persistent player credentials remain #26; this route stores no credentials. Network admission begins only after the effect remains active, so cancelled development effects cannot consume player seats.

The roster replaces its view from each validated public snapshot, preserving authoritative player order and stable ID keys. Names are the authoritative defaults until the phone identity slice (#14). Runtime-derived connectivity distinguishes connected players from disconnected seats retained through reconnect grace; recovery starts seats offline until their reconnect succeeds.

The first authoritative player is the host, matching `GameState::host`. The board enables Start only for its connected host, in the lobby, when the selected ruleset minimum player count is met. It sends the existing authenticated, sequenced Start command. A successful datagram write does not confirm the game: only an authoritative playing snapshot removes the pending state. Delivery errors or a five-second confirmation timeout keep the current lobby and offer an explicit retry. There is no automatic command retry. The Rust session remains the authority for phase, host, minimum count, and dealing.

An explicit Reconnect action resumes the current in-memory `BrowserMatch` capability and sequence cursor after a connection ends. Existing public state remains visible while reconnecting. Private rack bytes are decoded only for recipient validation; only the public projection reaches the shared board.

Chromium acceptance uses the actual Scrabble host: creation-response loss, stable route/reload, live joins, repeated roster snapshots, host/minimum restrictions, a deliberately lost Start command, confirmed start on two connections, and public disconnect evidence. Native restart coverage verifies that game state and private racks survive while restored connectivity changes correctly. Full checks: `bun run check`. Shared convention revision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
