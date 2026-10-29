# moenarch.com launch links

The launcher at `https://moenarch.com/scrabble.html` coordinates a public lobby
reference with the authoritative Rust Scrabble service. Scrabble gameplay,
private racks, board ownership and reconnect capabilities remain in Scrabble.
The setup service stores only a short-lived public match reference. Public
hosting is established by #67; integrated launcher publication and live
acceptance remain #80 until independently verified after deployment.

## Host and player routes

The host creates a setup lobby, then opens a board popup at:

```text
https://scrabble.moenarch.com/scrabble/?launch=moenarch&session=0123ABCDEFGH&view=board
```

The board origin is `https://scrabble.moenarch.com`, the same site as the Moenarch
account service, so launched boards can link libraries (#206). The client is served
under `/scrabble/` because that is its fixed Vite base; `/` redirects there. The
GitHub Pages build at `https://moritzbrantner.github.io/scrabble/` stays a guest-only
demo and is not a launch target.

This explicit board route offers **Create game**. The popup creates its match
through Scrabble's existing idempotent creation API and keeps the private
creation proof and board reconnect capsule in their existing tab-owned
boundaries. After creation it adds the canonical match ID to its own URL. Only after an
authoritative snapshot confirms that this connection owns the board does it
report `{type:"scrabble-match-created",session,match}` to its opener,
using the fixed target origin `https://moenarch.com`. Reloading a linked board
can repeat that public report only after its private reconnect capsule has
restored authoritative board ownership, allowing an identical binding retry.
An HTTP creation response or a public match URL alone never publishes a binding. Rematches
remove the original `launch` and `session` fields and never replace the original
lobby's immutable reference.

The launcher accepts reports only from the fixed board origin
`https://scrabble.moenarch.com` (launcher side:
[multiplayer-setup-service#71](https://github.com/moritzbrantner/multiplayer-setup-service/issues/71)), the exact board
window it opened and its own session. It registers the reference with the
existing host participant capability; that capability remains in the launcher
page's memory. See upstream [issue #44](https://github.com/moritzbrantner/multiplayer-setup-service/issues/44)
and its repository-owned acceptance contract. Identical registrations succeed;
conflicting registrations cannot overwrite the binding. Public GET does not
create a game or disclose credentials.

Guests open the public launcher invitation, resolve the reference and choose
**Open Scrabble player**. The resulting destination is:

```text
https://scrabble.moenarch.com/scrabble/?launch=moenarch&session=0123ABCDEFGH&match=b_<16 lowercase hex>_<32 lowercase hex>&view=player
```

The 12-character session uses the setup service's canonical alphabet. It is
public context, never a Scrabble match identity or seat credential. Canonical
match IDs are checked through the existing Scrabble join API and private
WebTransport admission. A bare session-only link fails closed; only the
explicit `view=board` creation route permits host creation. Duplicate, malformed
or incompatible routing identities fail before admission.

The player invitation and QR retain only supported public routing fields,
match identity and optional language. They never forward creation proofs,
participant tokens, reconnect capabilities, arbitrary query fields or URL
fragments. Following a rematch also removes the original lobby context from
the phone route, because that context belongs only to the finished match. A canonical board match invitation continues to work after setup
lobby expiry; the board does not require another setup lookup to reconnect to its
independent, longer-lived Scrabble match. An expired launcher invitation itself
cannot resolve a discarded binding and reports that failure. Reloading the
launcher loses its in-memory host capability and resolves an existing binding
without creating another match. A fresh tab cannot claim a board from a public
URL alone.

## Verification and deployment

The ordinary browser suite checks malformed links and independent players
joining/reconnecting to a real Rust match. The separately authored
[integrated acceptance](moenarch-integrated-acceptance.md) exercises the actual
built launcher, setup service, built web client (served at the board origin) and real WebTransport game,
including a committed word, reconnect, rematch and setup expiry. Its controlled
fixture establishes local behavior, not physical-device or public deployment
acceptance. Upstream acceptance separately covers capability authorization,
concurrent binding, conflicts and forged popup messages.

The moritzbrantner/moenarch.com host deploys the setup service, the Scrabble server
and the Scrabble web client at `scrabble.moenarch.com` (see
[production deployment](production-deployment.md)).
Verify the actual public launcher → board → two players → word → reconnect
journey against both deployed revisions before closing #80. Preserve the
operator-owned TLS keys, seed, recovery state and dictionary content; none
belongs in static assets or cross-origin messages.
