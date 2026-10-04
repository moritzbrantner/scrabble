# Game creation and retirement

The development entry point supports `?server=<HTTPS WebTransport base>&api=<HTTP(S) creation API base>`. Without a `match` parameter it offers **Create game**. Successful creation supplies a public join link and connects the shared board. Existing `?server=...&match=...` links still work. For local self-signed TLS, append `&certificate=<64 lowercase hex SHA-256 fingerprint>` from `bun run dev:tls`; fingerprint trust is accepted only by the development entry point. Production uses ordinary certificate trust and the prepared public endpoint configuration; actual public hosting is deferred to #67.

For example, after starting the default local server and Vite:

```text
http://localhost:5173/scrabble/?server=https%3A%2F%2F127.0.0.1%3A4433%2Fgame&api=http%3A%2F%2F127.0.0.1%3A8081&certificate=<public-fingerprint>
```

`SCRABBLE_BOARD_ORIGIN` must match the exact Vite origin, including hostname and port. The game API listens separately from read-only runtime status. Local HTTP is supported; production needs HTTPS termination at its gateway. Set `SCRABBLE_MATCH_IDS=` to start with zero matches. Legacy configured IDs without a creation timestamp remain useful for compatibility fixtures and are outside the created-game expiration policy.

## Public requests

`POST /games` requires `Content-Type: application/json` and this strict versioned body:

```json
{ "version": 2, "requestId": "0123456789abcdef0123456789abcdef", "requestedAt": 1791070000 }
```

The board generates 16 cryptographically random bytes and a Unix-seconds timestamp once per creation intent. Retries reuse both fields. The server accepts requests less than 120 seconds old and at most 10 seconds ahead of its clock. Invalid/future requests return 400; expired requests return 410 even if the game still exists. An expired request requires an explicit new intent; the board never silently allocates another game after losing a response.

Version two uses the `scrabble/create/v2` HMAC domain and `b_` match namespace to keep the shared board outside the playing roster. The board proves its role using the private creation nonce; it receives no playing rack. Version one remains accepted with its unchanged `scrabble/create/v1` domain and `g_` construction rules for old retries and replay histories. See [shared board authority](../adr/0007-shared-board-authority.md).

A domain-separated HMAC-SHA256 under the persisted private initialization key derives the ID's 128-bit opaque suffix. The canonical ID also embeds the original timestamp, allowing the factory to reconstruct lifecycle inputs during replay. It is independent of player/reconnect credentials. Successful first creation and duplicate retry both return 200:

```json
{
  "version": 1,
  "matchId": "b_000000006ac18f30_0123456789abcdef0123456789abcdef",
  "gameId": "123456789",
  "matchPath": "/game/matches/b_000000006ac18f30_0123456789abcdef0123456789abcdef",
  "expiresAt": 1791091600
}
```

Only these five public fields are returned; IDs and dates above are illustrative. `gameId` is a decimal string to preserve unsigned 64-bit precision in JavaScript. The browser validates the response shape and verifies that `matchPath` addresses the configured transport base and returned ID. Private racks, shuffle initialization and reconnect tokens are never creation data.

Placement and ticks belong to upstream `LiveMatchHost`. A match is reachable through its canonical route before the response returns. Concurrent/retried requests allocate one runtime and one slot; retries still succeed at capacity. A new request at capacity returns 429. Creation before serving or during drain returns 503, including duplicate requests during drain. Failed placement preserves its candidate runtime. The application holds only the factory key, trusted host handle and request serialization lock, with no secondary game-state or idempotency table.

## Lifetime and retirement

Created games, including abandoned games, expire six hours after their request timestamp. Finished games may retire after 6,000 deterministic ticks (five minutes at 20 Hz) from completion, with a minimum five-minute age from creation. Completed-phase behavior becomes reachable when the ordered finish-game slice is implemented; its lifecycle metadata is already part of canonical replay. A single sweeper checks eligible games every five seconds while the server is running. On restart it resumes from recovered metadata, so process downtime cannot reset the six-hour limit. This policy requires a correctly maintained server clock.

`DELETE /games/<match-id>` requests the same eligible retirement; it cannot terminate a fresh or still-retained game. It returns 204 on success, 409 when not eligible, 404 when unknown/already retired, and 503 during drain. Eligibility is monotonic for terminal games and hard lifetime expiration. The host freezes the runtime, fences old commands/reconnects, closes active connections and stops ticking before releasing the capacity slot. Retired IDs disappear from status and browser routes. Retirement discards that game's state; later creation retries are already expired and cannot recreate it.

Canonical tick history uses the runtime’s private SHA-256 digest checkpoints while recovery retains a full final canonical snapshot. At 20 Hz the six-hour limit yields at most 19,440,000 encoded tick-record bytes, plus commands, session events and the final snapshot, under the runtime’s 256 MiB recovery-image limit. A real Scrabble regression checks this bound.

Lifecycle is stored only inside `ScrabbleSimulation`'s canonical snapshot. Initial lifecycle inputs derive from the canonical match ID, so replay/recovery reconstruct the same authority. Public player snapshots do not expose lifecycle internals. Graceful recovery's bounded manifest supplies created IDs to the factory; configured defaults never recreate retired IDs. Hard termination remains outside graceful recovery guarantees.

## HTTP bounds and evidence

Headers are limited to 4 KiB, bodies to 512 bytes, concurrent requests to 32, and request/response work to five seconds each. Duplicate framing headers, transfer encoding, malformed/oversized bodies and trailing buffered data fail before game operations. The configured origin is the only browser origin accepted; origin checks are not authentication. Non-browser requests can omit `Origin`. Creation requests are not retained in cookies, browser storage or URLs. Board and phone reconnect capsules use separate tab-scoped session-storage keys; each contains only its version, canonical route, authenticated actor ID, reconnect token and next-command sequence state. No canonical snapshots, rack contents or creation nonces enter storage.

Real-process tests cover zero-match startup, concurrent retries, public-only responses, immediate canonical admission, origin/capacity/drain rejection, and retry/reconnect after restart. A real-runtime Scrabble test advances the supplied clock to retirement, then proves active-session closure, admission/reconnect rejection, freed capacity and expired-retry refusal. Chromium deliberately loses a successful creation response, recovers the same game from the board UI, and renders its live public board. Protocol, lifecycle projection/replay and malformed HTTP framing tests cover the remaining boundaries.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
