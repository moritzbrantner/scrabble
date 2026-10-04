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
