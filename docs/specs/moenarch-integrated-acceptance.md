# Integrated moenarch launcher acceptance

This is **retrospective independent acceptance** for existing PR #86 and issue
#80, not a claim that its original implementation followed tests-first ordering.
The acceptance context inspected preimplementation baseline
`1f56a01e570825890a5916d720d655527e8c4001` using `git show`, including the existing
creation, reconnect, shared-board and rematch public test seams, without reading
the existing PR86 launcher implementation or tests. New tests were independently
derived before further production edits. The source tested for the first red
observation is merge head `e24125d` (PR86 with current main merged).

Authoritative intent: issue #80, upstream multiplayer-setup-service #44,
`docs/DECISION-ATLAS.md` Decision 2.5 (distinct public shared board and private
player roles), `docs/specs/game-creation.md`, `docs/specs/phone-joining.md`,
`docs/specs/rematch.md`, and AGENTS.md. Shared convention sourceRevision:
`5ad0db36974c8b0e5f9812c90ee34a4f227852c3`.

## Contract

The actual moenarch launcher opens the trusted Pages board with
`launch=moenarch&session=<publicLobbyId>&view=board`. Without a match, this
host-owned popup must offer **Create game**, retain its own private board claim
proof, and report only `{type:"scrabble-match-created",session,match}` to its
fixed `https://moenarch.com` opener after successful authoritative creation.
No creation nonce or reconnect/participant capability crosses that boundary.
The launcher independently validates origin, exact popup source, session and
public canonical ID before binding with its in-memory host capability (#44).

Guests resolve that immutable public binding in the launcher and open Pages
player links for exactly that match. Pages treats the match as the canonical
authoritative destination; session is a public launcher hint, not a substitute
for the Rust match ID or an authorization credential. Canonical invitations
and existing reconnect identity remain usable after the short-lived setup
lobby expires. No new setup lookup may make an otherwise valid canonical
Scrabble invitation unusable after expiry.

The board remains outside the playing roster. Two independent player contexts
choose names and receive distinct identities/private racks. Only the board
starts the game; one valid AT move converges to board and both player editors;
refresh preserves identity and game state. A rematch is a separate canonical
game: remove original `launch` and `session` from its board URL and player
invites and preserve the original immutable launcher binding.

## Executable evidence

Run from the Scrabble repository:

```sh
SCRABBLE_SETUP_REPOSITORY=../multiplayer-setup-service \
  python3 ~/.local/share/coding-agent/disk-maintenance.py run -- \
  bun scripts/test-launcher-browser.ts
```

`scripts/test-launcher-browser.ts` provisions the real local setup binary,
builds the actual launcher, starts Scrabble's existing real Rust
`browser_game_fixture`, and builds the actual optimized Pages frontend with
its local WebTransport endpoint and test certificate fingerprint. It fails
explicitly when its upstream checkout is missing. The public HTTPS origins
are provided by the browser test's local gateway: it serves **actual artifact
bytes** and transparently forwards public HTTP APIs to the actual Rust
services, preserving methods, request bodies, headers, origin and responses.
It does not fabricate game creation, binding, join, admission, snapshot, move,
reconnect or retirement responses. Actual WebTransport UDP connects directly
to the real native fixture. Thus no public internet/service is required.

`apps/web/acceptance/launcher-integrated.spec.ts` and
`scripts/launcher-acceptance.config.ts` exercise host launcher → board creation
→ immutable binding → two independent players → legal move → reconnect →
rematch without binding replacement → actual 60-second setup expiry → original
canonical player reconnect. The native deterministic opening fixture is used
only to make a valid AT turn reproducible. It is a fixture workload, not real
remote-device acceptance. Traces/screenshots are disabled because browser
sessions contain capabilities. A disposable-wrapper run cleans Rust build
output, process directories and test build output on completion/failure.

This opt-in cross-repository tier supplements the repository's full
`bun run check` gate. It requires the upstream checkout at the candidate
revision; record both SHAs. Deterministic affected-test mapping is incomplete,
so run every acceptance and the full applicable gate before merge. Preserve
existing periodic full-suite CI coverage. Existing upstream #44 acceptance
remains responsible for authorization, origin/source/session rejection,
concurrent writes, conflicts and bounds.

## Initial observations and remaining acceptance

The first real integrated run on `e24125d`, with the new upstream candidate
launcher and native setup/game processes, reached the actual Pages board popup.
It failed at **Create game**: the application displayed “This moenarch.com
session has no Scrabble match. Ask the host for a game link.” No match was
created. This is an evidenced missing host-board adapter, not a mock-only
failure. Type checking, focused test/harness lint and formatting passed.
Assertions after that point, including rematch and expiry, remain unrun until
the adapter is implemented; no end-to-end pass is claimed.

After merge/deployment, verify exact published revisions and the complete
owner journey through actual moenarch.com and public Pages using independent
browsers/devices. Real DNS/TLS, network routing, remote UDP WebTransport,
physical phones and the public post-merge deployment remain outside this
local gateway result. Record **Awaiting live deployment verification** on #80
before requesting merge and keep/reopen it until actual public acceptance is
recorded. Public endpoints existing or a green local fixture do not close #80.

## Independent local-network permission correction

The first adapter candidate reached authoritative creation but public Pages
origin → loopback native WebTransport failed with “Unable to connect”.
Chromium gates that local-fixture topology behind local-network permission.
Granting `local-network-access` **only to the trusted github.io origin in the
three test browser contexts** made the entire real journey pass, including
legal AT, distinct identities/private racks, reconnect, rematch without
replacing the original binding, and actual setup expiry followed by canonical
player reconnect. TLS verification and the explicit test certificate fingerprint
remain intact; production application code receives no permission override.
The observed diagnostic run used Bun 1.4.0 and is not exact-toolchain acceptance.
Final candidate verification uses Scrabble's pinned Bun 1.4.2; the optional
`SCRABBLE_SETUP_BUN` executable override lets the runner build the upstream
launcher with that repository's separately pinned Bun 1.4.0. Record that final
run and both source revisions on issue #80 rather than inferring public live
acceptance from this local gateway result.

## Independent regression handoff after targeted review

Review comments 4274442202 and 4274442214 require confirmed board authority
before a trusted popup publishes a binding. New native integrated negative
cases prove an actual successful HTTP creation followed by unavailable browser
WebTransport leaves the lobby unbound, and a trusted original popup navigating
to an existing public match without its board capsule cannot bind it. Only the
browser transport is deliberately unavailable in the former case; game creation
and every server response remain real. Both cases require public-reference
GET 409 and no published match message. A separate positive case confirms an
owned board reconnect can re-report the same binding. That reloaded document
is deliberately not asked to start a rematch: the private creation nonce must
remain lost on reload under the existing rematch specification.

Review comment 4274442222 adds actual player **Join rematch** to the long
native journey: both new board and followed player URLs/invites must omit the
original launch/session hints while the original binding stays unchanged.
Review comment 4274442232 fixes the existing native-opening helper to resolve
its executable from `CARGO_TARGET_DIR`; the long journey deliberately supplies
a stale opening so the real native refresh branch runs even in fast tests.
The disposable target, executable, fixture directory and native freshness
validation remain real rather than a mocked helper result.

The pre-fix source is `36a0399`. Independent red observations: phone follow
retained `launch`/`session`; unavailable transport still published an immutable
binding after actual HTTP 200 creation. Owned reconnect was green. The initial
unowned-case selector was corrected to the established recover-board-session
error copy before assessing its binding result. These tests/harness changes
precede production fixes; no production code was edited by their author.
The corrected filtered native negative run reached the binding assertions in
both cases and failed with actual HTTP 200 instead of expected 409 (two tests,
about 0.5 seconds each after builds). This confirms both ownership defects
independently of error-message selectors.
