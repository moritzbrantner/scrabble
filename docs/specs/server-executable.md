# Authoritative server executable

The executable hosts the Rust Scrabble simulation through the prepared hosted `game-server` runtime. Current commands are start, draft preview/cancel and pass; commit, exchange, full word dictionary and the player shell retain their ordered follow-up issues. It is a real serving/recovery composition, not a complete playable MVP yet.

## Local development

```sh
cp .env.example .env
bun run dev:tls
bun run dev:server
```

Bun loads `.env` for these commands. Running the compiled Rust binary directly reads process environment only. `dev:tls` explicitly creates a short-lived self-signed localhost identity or reuses both existing identity files without rewriting them. It refuses production mode and incomplete identities. Its JSON output contains the public certificate SHA-256 digest for `BrowserMatch`'s development `serverCertificateHashes` option. It never prints the private key or shuffle seed. For renewal, stop the local server and explicitly replace the expired local certificate/key together. Production must use a trusted certificate and browser endpoint configuration added in #31.

By default the WebTransport base is `https://127.0.0.1:4433/game`, with match routes `/game/matches/table-1` and `/game/matches/table-2`. The status listener is `http://127.0.0.1:8080`; the game creation API is `http://127.0.0.1:8081`. The runtime binds all interfaces; keep local ports firewalled and expose status only to a trusted management network.

The installed English fixture ruleset is still the authored test configuration. Display names default to runtime player IDs until #12 adds join/name workflow. Endpoint discovery and player/spectator surfaces remain their later issues.

## Environment contract

[.env.example](../../.env.example) is the complete supported key catalog. Unknown `SCRABBLE_*` variables fail to prevent silently ignored typos. Values are validated once before constructing simulations or listeners.

| Variable                                | Default / constraint                                                                                        |
| --------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `SCRABBLE_MODE`                         | `local` or `production`                                                                                     |
| `SCRABBLE_PORT`                         | `4433`; 1–65535, UDP/WebTransport                                                                           |
| `SCRABBLE_API_PORT`                     | `8081`; 1–65535, TCP/game creation; different from status port                                              |
| `SCRABBLE_BOARD_ORIGIN`                 | `http://localhost:5173`; one canonical HTTP(S) origin; explicit HTTPS in production                         |
| `SCRABBLE_STATUS_PORT`                  | `8080`; 1–65535, TCP/read-only status                                                                       |
| `SCRABBLE_CERT_PEM`, `SCRABBLE_KEY_PEM` | `.local/server/cert.pem`, `.local/server/key.pem`; distinct files                                           |
| `SCRABBLE_SEED_FILE`                    | `.local/server/seed`; private 32-byte binary initialization key                                             |
| `SCRABBLE_RECOVERY_DIR`                 | `.local/server/recovery`; upstream-owned consumable recovery bundle                                         |
| `SCRABBLE_ROUTE_PREFIX`                 | `/game`; canonical absolute ASCII base path                                                                 |
| `SCRABBLE_MATCH_IDS`                    | `table-1,table-2`; unique canonical IDs, comma-separated without whitespace; empty starts with zero matches |
| `SCRABBLE_MAX_MATCHES`                  | `16`; 1–64, at least the configured match count                                                             |
| `SCRABBLE_RECONNECT_GRACE_TICKS`        | `1200`; 1–72000 deterministic ticks, at 20 Hz                                                               |
| `SCRABBLE_DRAIN_GRACE_MS`               | `500`; 0–30000 milliseconds                                                                                 |
| `SCRABBLE_TEST_SEED`                    | unset; optional 64 hex characters for local deterministic fixtures only                                     |

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
