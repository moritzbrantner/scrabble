# Independent predefined-match acceptance (#94)

Authority: `DECISION-ATLAS.md` 1.25, 1.31, 1.39 and current execution priority
4.1; `predefined-match-setup.md` at documentation commits `3803ef6` / `6ace017`,
against implementation baseline `72cf2cd19216533ee3abf6244875af81398f72ee`.
This is an independently authored acceptance contract, not implementation or a
second product decision source. Production behavior has not been changed.

## Representative inputs and existing seams

`crates/scrabble-server/tests/support/profile_catalog.rs` authors an explicit
operator catalog: unchanged legacy words, all 676 ordered ASCII two-letter
pairs, and a deliberately incompatible `ÖL` list; unchanged classic tiles and
an alternative with doubled nonblank values plus two extra A tiles; unchanged
classic 15×15 premiums and a 7×7 board with a triple-word center. These are test
data, not official word lists or production defaults. The alternative pair
list permits an actual native two-tile full-rack move without replacing the
random deal, rack, engine, validation, scoring, runtime or transport.

The browser fixture keeps the old `opening()` search and both native creation
nonces exactly intact. Development defaults retain the exact committed
`scrabble-game/fixtures/words.txt`; Pages defaults retain the existing `TA\nCTA\n`
bytes. Tile/premium/rule defaults remain exactly equivalent to their previous
complete deployment configuration. No fourth profile or new game-rule bounds
are introduced.

Only the test example may adapt to an older binary lacking the two introduced
configuration keys: an exact `unknown server configuration key` error for
`SCRABBLE_PROFILE_CATALOG_FILE` or `SCRABBLE_MATCH_CONFIGURATION_DIR` removes
only those keys and revalidates the original configuration. Every other error
propagates. It writes this effective configuration to `fixture-config.json`,
so `--fresh-opening` uses exactly the same legacy native factory. New tests
still require actual `/profiles` = 200 and real controls; there is no success
fallback or replacement API/game. A candidate recognizing the keys exercises
the real catalog path. The native acceptance does not use this compatibility
branch.

## Protected native contracts

The four `profiles_` tests in `server_process.rs` use the real executable,
HTTP/CORS interface, authenticated loopback WebTransport, actual snapshots,
public `MatchFactory`, upstream `RecoveryImage` and `verify_replay`:

- Catalog output uses immutable source identities and safe metadata; it does
  not disclose operator paths, dictionary contents or private creation state.
  Exact legacy-compatible defaults are independently checked.
- Missing/unknown/stale/incompatible selections and invalid settings reject
  with typed errors and zero match allocation. A later valid request with the
  same nonce remains usable. Concurrent identical retries recover one match;
  changing any source or rules under that key conflicts without mutation.
  Each source also varies alone while the other two remain fixed.
- Two real concurrent matches differ in tile values/counts, board/premiums
  and rack size. Their actual authoritative settings and identities differ,
  while the selected dictionary agrees. A selected two-player capacity is
  enforced in the lobby. Exchange minimum 1000 rejects a real exchange while
  retaining turn/rack; a two-tile native opening scores actual selected tile
  points × selected triple-word premium + configured bingo 17. Two scoreless
  turns finish the match. The other match remains unchanged.
- After shutdown, edit operator word/board sources. Fresh factory canonical
  replay of both recovery logs and actual restarted, token-authenticated
  snapshots must preserve original configuration, dictionary, phase/history,
  board and private rack. This does not prove recovery from deliberately
  corrupted/missing initialization files; fail-closed behavior for those
  remains the specification and a review obligation.
- A genuinely non-equivalent compact default reports `legacyCompatible=false`
  and explicit v3 creation reaches that compact configuration. Exact original
  legacy v2 creation retains its original complete identity and configuration.

## Bounded request and observation envelopes

The old 512-byte HTTP request cap cannot carry the actual full v3 identities
plus six rule fields. A protected positive fixture uses real fingerprinted
source identities with their maximum 64-character names, and a bounded stale
request uses 192-character revisions to reach source validation rather than a
size error. Canonical JSON at the declared field-width maxima is 1,124 bytes
(three 64/192 identity pairs, 32-character creation nonce, u64 timestamp and
six maximum-width numeric fields). The acceptance envelope is **2 KiB**: it
covers that exact shape with normal serialization headroom while rejecting a
body containing an additional 2 KiB padding field as 413 `body-too-large`.
A small extra field still fails strict decoding as 400 `invalid-request`.
Neither failure allocates a match. Existing header, timeout, CORS and input
validation contracts remain authoritative.

The API response observer uses the status helper's bounded 128 KiB envelope
for public tile/premium catalog metadata rather than the old 8 KiB read cap.
This is a test reader bound, not a new production catalog-size guarantee.
Baseline does not expose catalog output, so no observed response-size claim is
made before implementation.

## Real browser contracts

`apps/web/browser/profile-creation.spec.ts` is explicitly included in the
existing production-like Pages `testMatch` and is also discovered in development.
It uses the actual native fixture and forwarding snapshot observer:

- Untouched equivalent defaults still send the exact v2 three-field request.
- Three separate named selectors preserve one another; changing interface
  language changes none. Exact rack/bingo/exchange/scoreless/player values
  reach actual concurrently started matches joined by independent contexts.
  A real move proves native selected scoring, and reload preserves the match.
- Invalid combinations retain the chosen profiles and advanced edits without
  allocation. Restoring a valid choice, losing a real successful HTTP response,
  then retrying sends the identical v3 body and allocates only one match.
- An explicit rule edit restored to its old value still sends v3 with selected
  provenance, rather than silently falling back to legacy v2.

The untouched **frontend** `legacyCompatible=false` branch is not directly
covered by the global browser fixture, whose default must stay equivalent to
preserve the old AT/TA/CTA journeys. Native false-flag/v3 behavior is covered;
this frontend branch remains an explicit acceptance gap before feature completion.
A successful selector test does not prove physical-device or public deployment
acceptance, the custom libraries, hex mechanics or match clocks.

## Baseline evidence and final merge requirements

At production baseline `72cf2cd` (plus documentation-only `3803ef6` and
`6ace017`), final native execution reports three existing tests passing and
four new tests failing at strict rejection of the new configuration key, before
server readiness. Development browser execution reports nine passing tests
(eight existing creation tests plus untouched-default v2) and three new failures:
one absent Word list selector and two GET /profiles 404 responses. Pages reports
two passing tests (existing native AT→TA→CTA and untouched-default v2) and the
same three feature failures. Later profile, concurrency, scoring, retry and
recovery assertions are unrun on this baseline, not passing claims.

Clippy across all server targets, web type checking, web lint, web formatting,
Rust formatting and diff whitespace checks pass. Earlier Zod schema and reused
creation-nonce harness failures were repaired before these final browser runs;
they are not product findings. Full check was not claimed on this intentionally
red baseline. Logs are retained in `/tmp/scrabble-94-{native-final,clippy-final,
browser-dev,browser-pages}.log` in the author environment.

The final implementation
must run all four native contracts, all four browser contracts in both dev and
production-like Pages, existing creation/AT→TA→CTA checks, full `bun run check`
and deployment checks. Independently reverify any acceptance-fixture, harness
or expectation change. Production edits begin only after this committed
handoff. Shared convention source revision:
`5ad0db36974c8b0e5f9812c90ee34a4f227852c3`.
