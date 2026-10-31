# Turn notifications

Owner decisions: Atlas 2.11 (B, background push) and 2.14 (A, every subscribed authorized device). Issue #158.

## Behaviour

- A joined, non-forfeited player sees **Notify me when it is my turn** on their controller during a playing match. The browser permission prompt only appears after that deliberate press, never on page load. **Stop turn notifications on this device** removes only this device; other devices keep theirs.
- Unsupported browsers (no service worker, Push API or secure context) and denied permission show an explanation, and play is unaffected. The in-app "It is your turn" status stays the primary indicator. Background delivery depends on the platform. On iOS, Safari only supports web push for a site added to the home screen. Delivery is never promised where the platform does not offer it.
- The shared board is not a player and has no opt-in.

## Authority and privacy

- Opting in, opting out and fetching the server's VAPID public key all go over the transport-authenticated **reliable-control** stream (`push_key`, `push_subscribe`, `push_unsubscribe` JSON). The server binds each subscription to the native player identity of that connection, never to a claimed name, player id or link. A player can only remove their own devices. Endpoints must be HTTPS. Each player may have at most four devices per match, and the server holds at most 4096 subscriptions. Rebinding an endpoint moves the device rather than duplicating it.
- Subscriptions and the VAPID key live in the fixed `push/` child of `SCRABBLE_STATE_DIR` (mode 0600). They are separate from canonical match state, replay and recovery images.
- A server watcher reads each subscribed match's **authoritative** phase once a second. When the active player or turn changes after a commit, pass, exchange, timeout or forfeit, it sends one push to every device subscribed by the newly active player. The first observation only records the current turn, so opting in mid-turn announces nothing. The last announced turn is persisted, so restarts and repeated observations never repeat it. Finished or retired matches drop their subscriptions. If a push service returns 404/410, that subscription is removed.
- The payload is RFC 8291 `aes128gcm`-encrypted `{"kind":"turn"}`, authorised with an RFC 8292 VAPID ES256 token whose `sub` is `SCRABBLE_BOARD_ORIGIN`. It contains no rack, draft, credential, link or move. The service worker (`/scrabble/turn-notifications.js`) shows a generic localized message. On click it focuses an open game window or opens the route the device stored locally when it opted in.

## Evidence

- `crates/scrabble-server/src/push.rs` checks the published RFC 8291 Appendix A vector.
- `crates/scrabble-server/tests/push.rs` runs against a local push stand-in. It covers:
  - two devices of one player both receive a push that an independent receiver can decrypt, with a verified VAPID signature
  - the board and non-subscribers receive nothing
  - mid-turn opt-in sends nothing, and duplicates are suppressed
  - independent unsubscribe, and wrong-player unsubscribe has no effect
  - restart persistence, 410 cleanup, and drop on match end
  - insecure, malformed and excess subscriptions are rejected
- `apps/web/browser/turn-notifications.spec.ts` runs against the real native server with a stand-in `PushManager`, because headless Chromium has no push service. Ada opts in through the UI and the service worker registers. Ada's own turn ending produces no push, and nor does Lin's turn. Ada then closes her tab, Lin passes, and exactly one encrypted push reaches Ada's endpoint, which the test decrypts to the content-free payload.
- **Unverified:** delivery through real browser push services (FCM, Mozilla, Apple), the displayed notification and click-through on real devices, and the multi-device journey in the browser. A player identity is tab-scoped today, so two browsers cannot share one seat until the deferred Decision 2.15 control transfer exists. Multi-device fan-out is covered natively.
