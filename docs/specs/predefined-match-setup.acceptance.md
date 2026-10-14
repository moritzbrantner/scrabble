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

An additional independent acceptance context closes the untouched **frontend**
`legacyCompatible=false` gap before production edits. A separate invocation sets
`SCRABBLE_BROWSER_COMPACT_DEFAULT=1`; the test example changes only its actual
operator catalog's default board to the existing compact profile before startup.
The normal fixture, original words/opening search and default-v2 checks remain
unchanged. No API response is mocked. The fifth browser case reads the real false
flag, leaves the form untouched, checks the exact v3 source/settings request,
starts a real match and checks the native seven-square board, selected center
premium and rack size through actual host and guest snapshots. Separate dev and
optimized Pages invocations bind their clients to that invocation's actual server.
A default suite run explicitly skips this case because it requires the alternate
fixture; the two focused invocations below are mandatory before merge.
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
must run all four native contracts, the four normal browser contracts in both dev and
production-like Pages, existing creation/AT→TA→CTA checks, full `bun run check`
and deployment checks. Also run both alternate-default journeys:

```sh
SCRABBLE_BROWSER_COMPACT_DEFAULT=1 bun scripts/test-browser.ts --grep 'untouched non-equivalent'
SCRABBLE_BROWSER_COMPACT_DEFAULT=1 bun scripts/test-browser.ts --pages --grep 'untouched non-equivalent'
```

Both alternate-default baseline invocations against unchanged production fail
genuinely at the actual `/profiles` response: expected 200, received 404.
The false flag, request version and native match assertions are unrun at baseline.
Logs: `/tmp/scrabble94-compact-{dev,pages}-baseline.log`. Both disposable native
targets were cleaned. Type checking, web lint and formatting pass; no full gate
is claimed on the intentionally red baseline.

Independently reverify any acceptance-fixture, harness
or expectation change. Production edits begin only after this committed
handoff. Shared convention source revision:
`5ad0db36974c8b0e5f9812c90ee34a4f227852c3`.

## Independent review regressions at `702ba34`

Six additional real-process `server_process` contracts remove or corrupt durable
shared words after the first successful publication, then request either a new
configured match or the identical creation retry. The retry cases also remove or
corrupt its initialization manifest. A successful acknowledgment must have durable,
byte-identical inputs recoverable by a fresh factory; safe repair is permitted.
Otherwise a typed rejection must allocate no match and publish no new manifest.
Restoring the original inputs must allow the same request. Already-running games
may continue using their immutable in-memory dictionaries.

All six tests fail against unchanged production `702ba34`: HTTP success is returned
but a fresh factory cannot reconstruct the original match. The later restoration
assertions are unrun at baseline. Log:
`/tmp/scrabble94-live-durability-baseline.log`. An initial missing-import harness
compile error was fixed before this actual six-failure result.

The additional browser journey uses `SCRABBLE_BROWSER_EXCHANGE_REJECTION=1` to
append an operator tile profile containing one of each A–Z plus one blank. Two
real players with rack size 13 leave one tile in the native bag. The UI submits an
honest one-tile exchange; a test-only outgoing WebTransport fault changes that
single command to two distinct actually owned tiles. It preserves protocol kind,
sequence, game/turn identity and frame header, updating only payload length and
tile IDs. All inbound feedback, snapshots and authority come from the real native
server. This checks recovery from a correlated authoritative rejection, not a claim
that an honest UI naturally submits an invalid exchange.

After the native `ExchangeUnavailable` feedback, phone actions must recover within
one second, the real five-second timer must never replace feedback with a false
timeout, and an actual subsequent pass must advance native history. The ordinary
fixture and default profiles are unchanged. Run separately in both builds:

```sh
SCRABBLE_BROWSER_EXCHANGE_REJECTION=1 bun scripts/test-browser.ts --grep 'a real correlated exchange rejection'
SCRABBLE_BROWSER_EXCHANGE_REJECTION=1 bun scripts/test-browser.ts --pages --grep 'a real correlated exchange rejection'
```

Both development and optimized Pages baselines receive and display the actual native rejection, then
fails because Pass remains disabled after one second. Later unchanged-state,
timeout and subsequent-pass assertions are unrun. Log:
`/tmp/scrabble94-exchange-{dev,pages}-baseline.log`. These tests and documentation are
independent acceptance changes; production repair starts only after their handoff.
