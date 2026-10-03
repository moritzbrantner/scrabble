# Browser transport boundary

Status: accepted

## Decision

The browser implements the published `game-server` browser contract rather than a second Scrabble transport. `BrowserMatch` owns WebTransport, welcome admission, datagram sequence allocation, bounded snapshot reassembly, reliable control, cancellation, and the private reconnect capability. Its callbacks carry admitted player metadata and verified payload bytes. It has no dependency on React or game rules.

`game-protocol.ts` owns Scrabble payload validation independently of transport versioning. The view consumes validated public projections; private rack data is never inserted into public board props. Authoritative placement, scoring, dictionary decisions, and turn changes remain in Rust.

The upstream source dependency was initially pinned to `a3851dab9c1fb25dd31b465fb554ca475769caab`; the current pin is recorded in `crates/scrabble-server/Cargo.toml` and `Cargo.lock`. Rust-generated wire fixtures bind the browser to that revision's public constants and encoders. The source remains upstream-owned; no runtime source is copied here. The dependency is initially development-only for compatibility fixtures and real-network acceptance. The simulation adapter is the next slice.

## Consequences

Whole frames and fragmented frames share a strictly increasing snapshot delivery path. Reassembly follows upstream's pending-count, byte, fragment-count and idle-datagram bounds; incomplete snapshots do not stall later ticks. Tick and hash values remain `bigint` so u64 values never lose precision.

Reconnect tokens stay in JavaScript private fields for the lifetime of one client. Public status, callbacks, page URLs, and storage receive no token. The transport necessarily sends the token in the runtime's prescribed reconnect request path; browser-generated errors are replaced with fixed messages because those errors can contain the request URL. Captured browser network traces therefore remain ignored local artifacts.

Explicit reconnect reuses the sequence counter and rotates the token on welcome. Reload persistence and reconnect/backoff policy are intentionally owned by #26. There is no automatic command retry: an unreliable datagram can be lost and must never be presented as an acknowledged move.
