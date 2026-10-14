# Completed-game summary and rematch

Issue #33. [Owner decision 2026-10-14](https://github.com/moritzbrantner/scrabble/issues/33#issuecomment-6164645839): **one-tap follow (option A).** A rematch is a new match ([CONTEXT.md](../../CONTEXT.md)). The server keeps a finished→rematch link outside the finished match's game state and replay evidence. Only the shared board can start a rematch. The finished match stays immutable and read-only. The rematch gets a fresh bag, seed and turn order.

## Summary

When the authoritative public snapshot is `finished`:

- **Shared board:** the status still names the winner or tie (unchanged). The players list is headed **Final standings** and ordered by final score, highest first; tied players keep seat order. The bounded public turn history stays visible. No private rack contents or bag order are shown.
- **Phone:** the phone keeps showing "Game finished.", its own read-only rack and the final scores. If its own final rack still holds tiles, it also shows **Unplayed tiles: −N points**, where N is the sum of that rack's tile values under the match ruleset (blanks count zero). If the rack is empty, it shows **You used all your tiles.** The phone does not infer other players' adjustments.

## Starting a rematch (shared board)

A finished board offers **Start rematch** only while it holds the private creation nonce of the finished `b_` match in memory, that is, in the board tab that created it (or that created it as a rematch). The nonce never enters storage or URLs ([game creation](game-creation.md)). A reloaded board does not have the nonce. It shows **Start a rematch from the board that created this game.** instead of the action.

Starting sends:

```text
POST /games/<finished-match-id>/rematch
Content-Type: application/json

{ "version": 1, "boardRequestId": "<finished match creation nonce, 32 hex>", "requestId": "<new nonce, 32 hex>", "requestedAt": <unix seconds> }
```

The new `requestId`/`requestedAt` pair follows the creation-intent rules: it is created once per rematch intent, reused for retries, and accepted only within the existing 120-second window and 10-second skew. The rematch ID is derived exactly as version 2 board creation: `board_creation_id(requestId, requestedAt)`, a `b_` ID.

Server rules, in order:

| Condition                                                                          | Response                                                                                                    |
| ---------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Malformed body, wrong content type, unknown fields                                 | 400 `invalid-request` / 415 `json-required`                                                                 |
| New request too old                                                                | 410 `expired-request`                                                                                       |
| Service draining or not serving                                                    | 503 `draining` / `not-serving`                                                                              |
| Finished match unknown or retired                                                  | 404 `unknown-match`                                                                                         |
| Finished match ID is not `b_`, or `boardRequestId` does not verify against it      | 403 `board-rejected`                                                                                        |
| Finished match has not finished (lobby or playing)                                 | 409 `not-finished`                                                                                          |
| A rematch link already exists and was made from **this** `requestId`/`requestedAt` | 200 with the same join information (idempotent retry)                                                       |
| A rematch link already exists from a different request                             | 409 `rematch-exists`                                                                                        |
| No capacity for the new match                                                      | 429 `at-capacity`                                                                                           |
| Otherwise                                                                          | create the new match exactly like version 2 creation, record the link, 200 with its five public join fields |

Creation and link recording are serialized with ordinary creation, so concurrent retries yield one rematch and one link. Starting a rematch never submits a command to, or changes the snapshot, replay log or recovery evidence of, the finished match.

After success, the board replaces its URL's `match` with the rematch ID (as with creation) and shows the rematch lobby, which renders the new player invite and QR. The board connects with the rematch's own creation nonce, so it can start the rematch game and later its own rematch. On failure, it shows an alert and an explicit retry; retries reuse the same intent unless the server refused it or it expired.

## Following a rematch (phones)

```text
GET /games/<finished-match-id>/rematch
```

The response is 200 with the rematch's five public join fields once linked, 404 `no-rematch` while the match exists but has no rematch, and 404 `unknown-match` / 410 `expired-match` / 503 as for match lookup. The finished match's public invitees may read it, because the rematch ID is public join information. It allocates nothing.

A phone whose snapshot is `finished` checks this lookup about every two seconds until it finds a rematch or unmounts. Once found, it shows **Join rematch**. Tapping it:

1. moves the phone to the rematch's player route (`?view=player&match=<rematch-id>`, keeping only the validated development connection fields the invite builder allows; no name, token or other private value enters the URL), and
2. joins the rematch lobby as a new player under the phone's previous display name, without retyping it.

If admission fails (lobby closed, full or expired), the phone shows the normal join failure with the name entry prefilled. A player who never taps is simply not seated. Their finished view keeps working until that match retires.

## Link lifetime

The link lives in the server's in-memory game registry, next to placement, not in either match's simulation state. It is removed when the finished match retires (at the earliest five minutes after completion, see [game creation](game-creation.md#lifetime-and-retirement)). It is not part of graceful-restart recovery: after a server restart, both matches recover, but phones of the finished match can no longer discover the link. They can still join the rematch through its new invite on the board.

## Acceptance evidence

- Rust (real `Games` / HTTP boundary): every table row above, idempotent and concurrent retries producing one match and one link, the finished match's snapshot and replay unchanged by the rematch, a fresh rematch with a different game ID and dealing seed in the lobby phase, link removal at retirement, and `GET` results before and after linking.
- Chromium (built Pages client, real Rust server): finish a real multi-phone match; the board shows ranked final standings; a phone shows its unplayed-tile deduction; the board starts a rematch and shows the new lobby invite; one phone taps **Join rematch** and appears in the rematch lobby under its previous name with a different player identity route, while a phone that doesn't tap is not seated; the finished match still rejects gameplay; a reloaded board shows the explanatory note instead of **Start rematch**.
