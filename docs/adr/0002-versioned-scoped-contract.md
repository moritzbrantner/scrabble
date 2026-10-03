# Versioned JSON payloads and separate public/private projections

Scrabble uses strict, versioned JSON inside the game-server transport rather than embedding game-server framing in domain types. Numeric identifiers are canonical decimal strings so a browser can preserve the full transport identifier range without rounding. Public snapshots and an authenticated player's own rack are separate types; canonical state must never be serialized directly to a browser connection.
