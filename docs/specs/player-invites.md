# Player invitations

The board renders a player invite while its authoritative snapshot is in the lobby; the invite disappears once play starts. The invite points at the Vite base path (`/scrabble/`), with `?view=player&match=<canonical ID>`. Query routing loads the actual static index on GitHub Pages, so opening or refreshing the URL does not require an SPA path rewrite. A new URL is constructed from the current origin and base path, rather than forwarding the board URL. Production invites contain only the role and canonical match ID; credentials, creation retry keys, fragments, arbitrary parameters and fixture selectors are excluded.

Development may carry validated public server/API URLs and a public certificate fingerprint, permitting the actual local TLS runtime to be tested. Production ignores these query overrides and uses the public build settings `VITE_SCRABBLE_ENDPOINT` and `VITE_SCRABBLE_API`. Actual public deployment of those service settings is deferred to #67; #31 supplies the local game setup.

`qrcode` generates the symbol locally at quartile error correction. The React SVG contains only integer module paths, with an opaque black-on-white palette and a four-module quiet zone on every side. It is at least 384 CSS pixels on a laptop/TV and grows to maintain five pixels per module for denser local invitations; a narrow display scales it within the viewport. No remote image or QR service receives the URL. Copy player link writes the exact encoded URL to the clipboard. If permission fails, the same visible link remains available for manual copying.

The player route rejects malformed or ambiguous identifiers before network admission. `GET /games/<id>` reads current runtime membership and lifetime and returns the same five public join fields as creation, without allocating a match. Expired games return 410, unknown/retired games 404, and draining/unavailable service 503. The strict HTTP limits, origin policy, no-store response and request deadlines apply. A successful lookup is availability evidence, not an admission lease; retirement, capacity or connection failure may still reject the subsequent runtime admission. The route presents a useful failure and explicit retry without creating another game.

The player route opens the compact [phone join and identity flow](phone-joining.md); private-rack presentation remains #16. Phone refresh resumes its saved player capability. The board and phone use the [reconnect and resume](reconnect-resume.md) policy.

Validation: unit checks protect allowlisted URL fields and exact identifier round trips. Chromium rasterizes the actual rendered SVG and independently decodes it with the development-only `jsQR` dependency, compares that result with the visible link and clipboard, loads a player invitation, and verifies malformed/expired failure on direct load and refresh. The real runtime lifecycle test checks lookup availability and six-hour expiry. Full repository checks: `bun run check`. Shared convention revision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.

## Same-tab joining

An opened match offers **Join as player** in the current tab after the existing
public `/join` check confirms availability. This also works from a public board
URL in a tab without board ownership. Full, started, expired, unavailable and
connection failures show the existing admission reason and an explicit retry;
current lobby snapshots refresh the hint when the roster changes. Admission
still rechecks availability and the native runtime remains authoritative.

The action follows the canonical player invitation, retaining only public
routing context. It reuses the name form, authenticated identity and player
reconnect capsule; query names or player IDs never select a seat. The separate
board capsule remains in the tab. A player route holding that match's board
capability offers **Return to board**; browser history and refresh recover the
appropriate role independently. Returning to the controller resumes its own
player, including after play starts. The invite link, clipboard and QR remain
available for other devices.

Focused Chromium coverage in `same-tab-join.spec.ts` exercises these role and
history transitions, private rack preservation, duplicate-name isolation,
mobile touch joining and admission failure/retry states. This does not claim
physical-device or live-hosted acceptance.
