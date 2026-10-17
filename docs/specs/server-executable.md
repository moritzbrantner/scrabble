# Authoritative server executable

The executable hosts the Rust Scrabble simulation through the prepared hosted `game-server` runtime. Current commands are start, draft preview/cancel, commit, pass and exchange. A deployment can configure a local dictionary through the [word-list boundary](dictionary-validation.md); the small authored fixture remains the default. It is a real serving/recovery composition, not a complete playable MVP yet.

## Local development

### Local game setup

The current owner decision is local hosting. Scrabble runs its Rust simulation through `game-server`; it does not use the separate WebRTC `multiplayer-setup-service`. Install the repository's Bun/Rust dependencies and a local Docker Engine with Compose, then prepare the dictionary once:

```sh
mkdir -p .local/deployment
git clone --depth 1 --branch rel-2026.02.25 https://github.com/en-wl/wordlist.git .local/scowl-source
bun scripts/prepare-dictionary.ts .local/scowl-source .local/deployment/content
bun run dev:local
```

Dictionary preparation also needs Make, Python 3 and SQLite; see the [pinned preparation recipe](production-deployment.md#reproducible-initial-english-dictionary). If content is already prepared, reuse it instead of rerunning the commands that create it. An alternative prepared content directory can be passed as `bun run dev:local /ABSOLUTE/CONTENT_DIRECTORY`. The launcher checks its provenance hash and explicitly loads that dictionary; it never silently substitutes the small test fixture.

Open the **complete URL printed by the launcher**, including its connection query, to create a game. The same URL is stored in `.local/local-server/url.txt`. The plain Vite URL without that query remains a fixture preview. The board URL and its QR/player invitations advertise this computer's private LAN IPv4 address, so phones on the same Wi-Fi or Ethernet network reach this computer rather than their own localhost. Open the printed LAN URL on the board as well. The launcher ignores loopback and container bridges when choosing an address; if multiple physical interfaces are available, use `LOCAL_LAN_IP=192.168.1.10 bun run dev:local` with an address actually assigned to this computer. Physical-device acceptance remains distinct from automated browser evidence.

The launcher uses `compose.yaml` with `compose.dev.yaml` under the dedicated `scrabble-local` project. UDP 4433 for WebTransport and HTTPS 5173 for the browser client are reachable at the selected LAN address. TCP 8081 for the native API and TCP 8080 for management health remain loopback-only. Vite proxies same-origin `/api` requests to that native API. The exact allowed browser origin is `https://LAN_IP:5173`. Phone and desktop browsers need a secure context for WebTransport: on this trusted LAN, accept the local self-signed HTTPS certificate in the browser or explicitly trust it on the device. The certificate covers the selected IP; no global browser security flags are required. Local firewalls and Wi-Fi client isolation must allow access to HTTPS 5173 and UDP 4433 from participating devices. A remote Docker context or already-running local launcher is refused. Local trust uses the public digest of a short-lived self-signed certificate only in development; production trust requirements remain intact.

TLS and persistent seed/recovery storage live under ignored `.local/local-server`; the prepared content is mounted read-only. Ctrl+C stops the client and gracefully stops this project's server, preserving those files for the next run. Changing dictionary identity with existing recovery data is unsupported. Do not delete state to work around recovery failures. To renew an expired local certificate, stop the launcher and explicitly replace both `.local/local-server/tls-LAN_IP/cert.pem` and `key.pem` together before restarting. The local override disables automatic container restart so a stopped development server stays stopped.

### Hosting players on other networks

Atlas Decisions 5.1 and 5.3: a local checkout hosts with the same native server, so friends on other networks can join a game you run. Forward **TCP 5173** (browser client and its `/api` proxy) and **UDP 4433** (WebTransport) on your router to this computer's LAN address, then start the launcher with the public DNS name or IPv4 address those players use:

```sh
LOCAL_PUBLIC_HOST=play.example.com bun run dev:local
```

The certificate, the server's allowed browser origin, the printed game URL and therefore every invite link and QR code use `https://play.example.com:5173`. The services still listen on the selected LAN address that the router forwards to. Everyone, including players at home, opens the printed public URL; local devices need a router that supports NAT loopback or a local DNS entry for the name. There is no relay: when forwarding is missing or the host stops the launcher, remote players cannot connect.

Without a supplied certificate the launcher generates its short-lived self-signed certificate for both the LAN address and the public host (stored under `.local/local-server/tls-LAN_IP-PUBLIC_HOST`), and remote players accept it in their browser, as on the LAN. To avoid that warning, supply a trusted certificate issued for the public name, for example a free Let's Encrypt certificate:

```sh
LOCAL_PUBLIC_HOST=play.example.com \
LOCAL_PUBLIC_CERT=/etc/letsencrypt/live/play.example.com/fullchain.pem \
LOCAL_PUBLIC_KEY=/etc/letsencrypt/live/play.example.com/privkey.pem \
bun run dev:local
```

The launcher copies both files into `.local/local-server/tls-supplied-PUBLIC_HOST` at every start (so renewed certificates apply on restart and symlinked files mount into the container) and omits the self-signed certificate digest from the game URL. A supplied certificate requires `LOCAL_PUBLIC_HOST`. The GitHub Pages build is unaffected and keeps using the moenarch.com servers.

### Native development

```sh
cp .env.example .env
bun run dev:tls
bun run dev:server
```

Bun loads `.env` for these commands. Running the compiled Rust binary directly reads process environment only. `dev:tls` explicitly creates a short-lived self-signed localhost identity or reuses both existing identity files without rewriting them. It refuses production mode and incomplete identities. Its JSON output contains the public certificate SHA-256 digest for `BrowserMatch`'s development `serverCertificateHashes` option. It never prints the private key or shuffle seed. For renewal, stop the local server and explicitly replace the expired local certificate/key together. Production must use a trusted certificate and browser endpoint configuration added in #31.

By default the WebTransport base is `https://127.0.0.1:4433/game`, with match routes `/game/matches/table-1` and `/game/matches/table-2`. The status listener is `http://127.0.0.1:8080`; the game creation API is `http://127.0.0.1:8081`. The runtime binds all interfaces; keep local ports firewalled and expose status only to a trusted management network.

The installed English fixture ruleset is still the authored test configuration. Phones join with display names and private rack projections; a separate shared board owns Start.

## Environment contract

[.env.example](../../.env.example) is the complete supported key catalog. Unknown `SCRABBLE_*` variables fail to prevent silently ignored typos. Values are validated once before constructing simulations or listeners.

| Variable                                                   | Default / constraint                                                                                        |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `SCRABBLE_MODE`                                            | `local` or `production`                                                                                     |
| `SCRABBLE_PORT`                                            | `4433`; 1–65535, UDP/WebTransport                                                                           |
| `SCRABBLE_API_PORT`                                        | `8081`; 1–65535, TCP/game creation; different from status port                                              |
| `SCRABBLE_BOARD_ORIGIN`                                    | `http://localhost:5173`; one canonical HTTP(S) origin; explicit HTTPS in production                         |
| `SCRABBLE_STATUS_PORT`                                     | `8080`; 1–65535, TCP/read-only status                                                                       |
| `SCRABBLE_CERT_PEM`, `SCRABBLE_KEY_PEM`                    | `.local/server/cert.pem`, `.local/server/key.pem`; distinct files                                           |
| `SCRABBLE_SEED_FILE`                                       | `.local/server/seed`; private 32-byte binary initialization key                                             |
| `SCRABBLE_RECOVERY_DIR`                                    | `.local/server/recovery`; upstream-owned consumable recovery bundle                                         |
| `SCRABBLE_ROUTE_PREFIX`                                    | `/game`; canonical absolute ASCII base path                                                                 |
| `SCRABBLE_MATCH_IDS`                                       | `table-1,table-2`; unique canonical IDs, comma-separated without whitespace; empty starts with zero matches |
| `SCRABBLE_MAX_MATCHES`                                     | `16`; 1–64, at least the configured match count                                                             |
| `SCRABBLE_RECONNECT_GRACE_TICKS`                           | `1200`; 1–72000 deterministic ticks, at 20 Hz                                                               |
| `SCRABBLE_DRAIN_GRACE_MS`                                  | `500`; 0–30000 milliseconds                                                                                 |
| `SCRABBLE_DICTIONARY_FILE`                                 | unset; UTF-8 word list, at most 64 MiB; absolute path in production                                         |
| `SCRABBLE_DICTIONARY_NAME`, `SCRABBLE_DICTIONARY_REVISION` | unset; set with FILE; 1–64 ASCII letters, digits, hyphens, underscores or dots                              |
| `SCRABBLE_TEST_SEED`                                       | unset; optional 64 hex characters for local deterministic fixtures only                                     |

Production requires explicit absolute certificate, key, seed and recovery paths, plus explicit match IDs (which may be empty) and an HTTPS board origin. It forbids test seeds. Mount seed and recovery storage persistently; keep the seed outside the recovery directory and separate from TLS files. On Unix the seed must be a regular owner-only file; symlinks and group/other access fail. A corrupt seed or recovery directory without its seed causes startup failure without reinitialization. A failed interrupted initial key publication can leave `<seed>.tmp`; investigate the files and remove only that disposable temporary file explicitly before retrying.

## Runtime health and recovery

The app uses upstream status contract version 1 directly:

- `GET /healthz`, `/readyz`, `/status`
- `GET /matches/<id>/healthz`, `/readyz`, `/status`

Health/readiness and placement capacity are different facts. Unknown matches fail with 404; mutations are unsupported. Status contains process/match facts, never racks, seed material or reconnect tokens. The separate application API exposes [creation and bounded retirement](game-creation.md), plus [read-only public invite lookup](player-invites.md); reliable control remains rejected by default.

Send SIGINT or SIGTERM for graceful shutdown. The runtime marks readiness unavailable, drains, freezes and writes one recovery bundle for the current hosted matches. The next successful startup reconstructs the same factory/ruleset inputs, verifies/replays saved authority, restores runtime reconnect capabilities and sequence cursors, and consumes the bundle after the TLS listener binds and before marking serving ready. The persistent initialization key remains. Recovery restores manifest-listed IDs through the factory, including dynamically created games; configured initial IDs are used only on a fresh start. Do not reuse an incompatible ruleset with an existing bundle. A hard kill does not produce a new graceful recovery bundle; continuous crash persistence is outside this runtime contract.

## Verification

`cargo test -p scrabble-server --locked` exercises configuration, factory, simulation and a Unix real-executable test. The process test allocates OS-assigned UDP/TCP ports and temporary TLS/state files, uses actual WebTransport clients, hosts two independent matches, sends SIGTERM, restarts and reconnects with preserved racks/turns and fenced sequences. It needs neither a browser nor external services. The usual `bun run check` includes this evidence plus the existing Chromium adapter acceptance.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.

## Independent predefined match setup

The create-game form can select an operator-provided word list, tile profile and
square-board profile independently, plus advanced rack/scoring/exchange/player
rules. See [the catalog and initialization contract](predefined-match-setup.md).
`SCRABBLE_PROFILE_CATALOG_FILE` points to the strict source catalog; relative
word paths resolve beside that file. Without a catalog, the existing deployment
configuration remains available, including unchanged untouched v2 creation. A
catalog with `"includeBuiltin": true` starts from those built-in profiles (the
deployment word list, default and CH/SCH tiles, built-in square and hex boards and
their default selection) and appends its own rows; `builtinWordListLabel` names the
built-in word list, and `defaultSelection` and `boardProfiles` become optional. Without
`includeBuiltin` the catalog must list every kind and its `defaultSelection`. Source
names stay unique and each kind holds 1 to 32 rows, built-in rows included. The
generated word-list presets use this form (#153; see
[word-list presets](production-deployment.md#word-list-presets)).
Explicit edits use configured v3 creation and an opaque `p_` match ID.

When no operator catalog is configured and the deployment tiles exactly match
`english_fixture()`'s ordinary English distribution, the fallback additionally
offers `builtin-english-ch-sch` revision `1`, labeled **English + CH / SCH**.
It preserves all 100 ordinary tiles and blanks and adds two CH tiles worth 7
points and one SCH tile worth 8 points (the sums of their constituent values),
for 103 physical tiles. The existing SCOWL English dictionary is compatible;
the server still validates every selected combination. A deployment default
already named `builtin-english-ch-sch` retains that name without adding a
conflicting preset. Other custom tile distributions receive no added sample.
The classic default and its v2/`legacyCompatible` behavior remain unchanged;
operator catalogs are used exactly as supplied, without injecting this preset.

This implements Atlas 1.41's optional English sample, not proof of public
availability. Publish and verify the CH/SCH-capable frontend before exposing the
new server catalog, then complete the [actual hosted acceptance journey](public-multi-letter-preset.md).

`SCRABBLE_MATCH_CONFIGURATION_DIR` defaults to `configurations/` beside the seed.
In production use an absolute durable directory separate from runtime recovery.
Preserve it with the seed and recovery during upgrades. It holds authenticated
per-match initialization and shared immutable word versions, not private racks
or command authority. Deleting/corrupting required inputs fails recovery closed.
Source edits apply only to future selections. The existing v1 rematch API creates
a new deployment-default v2 game; configured-profile carryover is not promised.

`SCRABBLE_BOARD_LAYOUT_DIR` (#101) holds the durable guest-owned custom board
layout library. It defaults to `layouts/` beside the seed; the supplied Compose
file sets `/state/layouts` and the Hetzner environment `/var/lib/scrabble/layouts`.
In production it must be absolute and separate from the TLS files, seed, runtime
recovery and match initialization directories. The server creates it (mode 0700)
and stores one `l_<id>.json` file (mode 0600) per saved original: its name, an
optional share id, its recent immutable versions and an HMAC verifier of the
owner capability keyed from the private seed. The bearer capability itself is
never stored, so the library is unusable for owners if the seed is replaced; the
server therefore refuses to create a new seed while this directory is non-empty.
Back it up and preserve it together with the seed. Deleting it removes every
guest layout (owners cannot recover them, as the UI states) but never changes a
started match, whose composed board is pinned in its initialization manifest.
Layout routes (`/layouts…`, `/shared-layouts/…`) accept request bodies up to
32 KiB; all other API routes keep the 2 KiB limit. At most 4096 originals and 16
selectable versions per original are retained. Only the owner capability withdraws a share
(`POST /layouts/{id}/unshare`, #134): the old share link then answers 404,
sharing again issues a fresh id, and saved copies and started matches are
unaffected.

`SCRABBLE_TILE_PROFILE_DIR` (#100) holds the durable guest-owned private tile-profile
library. It defaults to `tile-profiles/` beside the seed; the supplied Compose file sets
`/state/tile-profiles` and the Hetzner environment `/var/lib/scrabble/tile-profiles`. In
production it must be absolute and separate from the TLS files, seed, runtime recovery,
match initialization, board-layout and word-list directories. The server creates it
(mode 0700) and stores one `t_<id>.json` file (mode 0600) per saved original: its name,
its recent immutable versions and a seed-keyed HMAC verifier of the owner capability,
never the capability. As for layouts, the server refuses to create a new seed while this
directory is non-empty. Routes are `POST /tile-profiles` and
`POST /tile-profiles/{id}/read|update|delete`, with bodies up to 16 KiB; tiles use the
catalog's `{face,count,value}` rows and the same validation as operator tile profiles.
There are no share links (Decision 1.27). A match selects a retained version by its
`identity` as `selection.tile`; at most 4096 originals and 16 selectable versions per
original are retained. Deleting the directory removes every guest profile but never
changes a started match, whose tiles are pinned in its initialization manifest.

`SCRABBLE_WORD_LIST_DIR` (#97) holds the durable guest-owned custom word-list
library ([contract](word-list-library.md)). It defaults to `word-lists/` beside
the seed; the supplied Compose file sets `/state/word-lists` and the Hetzner
environment `/var/lib/scrabble/word-lists`. In production it must be absolute and
separate from the TLS files, seed, runtime recovery, match initialization and
board-layout directories. The server creates it (mode 0700) and stores one
`w_<id>.json` entry (mode 0600) per saved original — name, optional share id,
version metadata and a seed-keyed HMAC verifier of the owner capability, never
the capability — plus one verified `w_<id>-r<revision>.txt` normalized word file
per retained version. As for layouts, the server refuses to create a new seed
while this directory is non-empty; back it up and preserve it with the seed.
Deleting it removes every guest list (owners cannot recover them, as the UI
states) but never changes a started match, whose word set is pinned in the
match configuration store. Word-list routes (`/word-lists…`,
`/shared-word-lists/…`) accept request bodies up to twice the configured byte
bound plus 4 KiB. The bounds are operator-configurable (Decision 1.50) and
published by `GET /word-lists/limits`: `SCRABBLE_WORD_LIST_MAX_BYTES` (default
1048576 per list, at most 64 MiB), `SCRABBLE_WORD_LIST_MAX_WORDS` (200000 per
list), `SCRABBLE_WORD_LIST_MAX_LISTS` (1024 originals and copies),
`SCRABBLE_WORD_LIST_MAX_VERSIONS` (8 versions per list selectable for new
matches, at most 64) and `SCRABBLE_WORD_LIST_MAX_STORED_BYTES` (268435456 bytes
of retained words). Each must be a positive decimal integer. Lowering a bound
never changes a started match.

For the supplied Docker Compose deployment, place an operator-authored catalog
and its word files in the existing read-only content directory. An explicit
operator Compose override can enable it without changing the default service:

```yaml
services:
  server:
    environment:
      SCRABBLE_PROFILE_CATALOG_FILE: /content/profiles.json
      SCRABBLE_MATCH_CONFIGURATION_DIR: /state/configurations
```

Pass that override after `compose.yaml` using Compose's `-f` option, including it
on subsequent start and update commands. Both paths use the existing content
and durable-state mounts. Host `SCRABBLE_*` environment variables alone do not
override the service's explicit Compose environment. Validate the catalog before
release; this repository's authored acceptance dictionaries are test data.

Release the frontend and server together: the three-selector frontend needs the
real public catalog API and configured-match routing. Public availability still
requires the feature's recorded post-deployment owner journey, rather than a
successful local fixture.
