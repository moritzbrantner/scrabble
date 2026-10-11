# moenarch.com launch links (integration boundary)

Status: local integration contract; **not evidence of a deployed moenarch.com launcher or public Scrabble host**. Public hosting remains #67; full launch integration remains #80.

The Scrabble game authority is the Rust `scrabble-server` over WebTransport, not `multiplayer-setup-service`. The latter's lobby ID is short-lived public rendezvous metadata. It does **not** identify, create, authorize or restore a Scrabble match.

## Public URL contract

For a launcher that has already selected one canonical Scrabble match for its lobby, open:

```text
https://moritzbrantner.github.io/scrabble/?launch=moenarch&session=0123ABCDEFGH&match=b_<16 lowercase hex>_<32 lowercase hex>&view=player
```

The 12-character `session` follows the setup service's canonical lobby alphabet, not its formatted display code. `match` must be the public ID returned by the Scrabble creation API. The ordinary Scrabble player invitation and QR use the same Pages base and retain only the validated public `launch` and `session` parameters, the match ID and optional interface language. No player/host capabilities, board creation request IDs, credentials, server addresses in production, URL fragments or arbitrary query parameters are forwarded.

A link with only `launch` and `session` fails closed: **the browser does not create a second Scrabble match or guess a mapping**. Repeated, malformed or incompatible launch fields also fail before any game admission. A linked match still undergoes the existing authoritative public `GET /games/<match-id>` check and private WebTransport admission. A lobby ID alone is never evidence of seat ownership.

An existing board can attach public launch context to its **own tab's** canonical match URL, then share the derived player invitation. A fresh browser cannot claim the board from its public URL: the board's proof and reconnect capability remain private and tab-scoped. Player reconnect credentials and sequencing remain separately owned by each player browser. Shared context is informational, not a new authorization mechanism.

## Required launcher-side authority (not implemented here)

The moenarch.com launcher must register or otherwise obtain exactly one immutable `lobbyId -> matchId` binding from an **authorized lobby host**, and use that binding for every participant and retry. The current setup service exposes no such binding API; its `GET /lobbies/<id>` only returns participant/expiry metadata. Scrabble cannot prove that two arbitrary links with the same public session ID refer to the same match, and it must not claim to do so.

A future service-side binding contract must:
- Authorize creation/confirmation using the **existing host participant capability**, never a public lobby ID; retain that credential only in the owning launcher process.
- Store only a bounded, short-lived **public Scrabble match ID**, not game state, player racks, credentials, or simulation history. Reject a conflicting second binding, including concurrent submissions.
- Return the same binding for duplicate retries; expose only the public match ID to guests; validate match identity against the Scrabble API before use.
- Treat expiry, launcher restart and match retirement explicitly. Invitations containing the canonical Scrabble ID can continue to reconnect through the authoritative server even after the short-lived signaling lobby is gone.
- Keep the launcher and Scrabble origins explicit; do not relay private WebTransport reconnect tokens via `postMessage`, lobby signaling, URLs or logs.

The binding schema and the launcher page that creates/renders it must be verified against the **actual running moenarch.com implementation** before considering this contract complete. This is a proposed additive Pages link contract, not a claim that the launcher currently emits it.

## Public server deployment remains independent

Do not set `deploy/client.json` until a real host satisfies [production deployment](production-deployment.md): stable DNS, trusted TLS matching the gameplay domain (including the direct UDP WebTransport listener), inbound TCP **and UDP** 443, persistent private seed/recovery state, licensed dictionary content, secure Compose configuration, Pages origin allowlist, successful health/readiness and real cross-device browser validation.

For local hermetic evidence, `apps/web/browser/moenarch-launch.spec.ts` creates a real Rust match, attaches public launch context in its already-authorized board tab, joins two independent browser contexts, submits a turn and reloads both a player and the board. Existing browser tests additionally cover lost creation acknowledgments, private snapshots, invalid reconnect capabilities and transport interruptions. The local test does not prove moenarch.com actually emits this link or that a public host is reachable.
