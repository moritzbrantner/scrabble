# Hosted Scrabble executable

Status: accepted

## Decision

The Rust executable composes `ScrabbleSimulation` with upstream's prepared hosted runtime. `game-server` owns routing, TLS/WebTransport, player session limits, epoch fencing, health/readiness, draining, recovery bundles and restart consumption. The app adds strict environment configuration, a small simulation factory and signal forwarding. It serves at least two named independent matches without importing browser UI code or implementing a second session stack.

The factory persists one private 32-byte initialization key outside the upstream recovery directory. New keys use OS-backed cryptographic randomness, owner-only permissions on Unix, and atomic no-overwrite publication of a complete synced file. Existing malformed, exposed or missing keys with recovery evidence fail closed. The runtime consumes its recovery bundle on successful restart; it cannot consume the initialization key.

Each match's deal seed is HMAC-SHA256 of its canonical match ID with a versioned domain separator. Its public numeric game ID derives separately from SHA-256 of the match ID, not from the secret key. Match order cannot change identity or initialization. Hash collisions in the configured game-ID set fail explicitly. The installed ruleset/dictionary identity and factory derivation are replay inputs; incompatible changes require an intentional migration rather than silently resetting games.

The executable now consumes upstream's dynamic prepared host. The factory is reusable for individual IDs, and recovery reconstructs the bounded manifest membership rather than requiring every restored ID to be preconfigured. Fresh starts still use configured IDs. A bounded application HTTP surface now translates creation and eligible-retirement requests into this boundary; gameplay and reconnect authority stay upstream. Retry identity is derived from a timestamped request and private MAC, rather than a second map of game state. Created-match lifecycle metadata lives in the canonical simulation snapshot. See [creation policy](../specs/game-creation.md).

## Operational boundary

Configuration defaults to local development with two matches, finite capacity/grace bounds and TLS file paths under ignored `.local/server`. It never disables TLS or supplies a predictable default shuffle seed. A local-only deterministic test seed can initialize a new private file, but cannot overwrite an existing seed and is forbidden in production. Production requires explicit absolute TLS, seed and recovery paths and match IDs (possibly empty), plus an explicit HTTPS board origin. Unknown keys and invalid configuration fail before serving.

Status is upstream's read-only HTTP surface, and reliable control defaults to rejection until the application requires a specific control operation. The runtime's listeners bind public interfaces; status must remain on a trusted management network and local development must restrict listener exposure through the host firewall. The creation listener also binds all interfaces. It enforces the configured board origin for browser requests; CORS is not authentication. Production must terminate HTTPS at its gateway.

SIGINT/SIGTERM are forwarded to the existing drain/recovery channel. The application owns and cancels its signal task when serving ends. Hard termination and crash recovery are not invented here: the upstream contract persists recovery during graceful shutdown, not continuously.

## Evidence

A real-executable loopback test verifies two independently admitted matches, private projections, isolated starts, runtime process/match status and readiness, SIGTERM recovery bundle creation, restart consumption, unchanged initialization key, rotated reconnect epochs, restored racks/turns, rejection of a previously used sequence and continued commands. Configuration/factory tests cover strict production requirements, deterministic initialization, secure file modes and refusal to replace damaged or lost authority inputs.
