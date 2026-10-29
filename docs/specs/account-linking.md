# Linking guest libraries to a Moenarch account

Issue #132, Decision 1.46. A player may link guest-owned word lists (#97), private
tile profiles (#100) and square/hex board layouts (#101) to the **shared Moenarch
account**. Scrabble does not authenticate anyone itself. It depends on the account
service (repository `moenarch.com`, `services/accounts`) only through that
service's HTTP session contract:

- The browser carries an opaque session token in the `moenarch_session` cookie.
- The Scrabble API verifies it with `POST <SCRABBLE_ACCOUNTS_URL>v1/sessions/verify
  {token}` → `{account_id, display_name, session_id, expires_at}` or `401`.
- Positive answers are cached for up to 60 seconds, so a sign-out or a revoked
  device takes effect within a minute. The browser never names an account id.

Scrabble stays the authority for the libraries, their permissions and sharing.
Linking records the verified `account_id` as the original's linked owner. It does
not move data out of Scrabble or change started matches, replays or recipient copies.

## Behaviour

- **Guest use is unchanged.** Without sign-in, or when the account service is not
  configured, every library works with its browser-held owner capability. If the
  service is unset, the API answers `503 accounts-unavailable` and the page shows
  no account section.
- **Link** (`POST /account/link {version,kind,id,ownerCapability}`). A link needs both
  a verified session and the guest owner capability. The original guest capability
  keeps working afterwards.
- **Duplicate link.** Linking again to the same account is idempotent
  (`linked: "existing"`).
- **Wrong owner.** An original linked to account A is never re-linked to B. That
  answers `409 account-link-conflict`, even when B holds the guest capability.
  Account B cannot reconnect it (`403 account-not-owner`) and does not see it in
  its listing.
- **Reconnect on another device** (`POST /account/libraries`, then
  `POST /account/reconnect {version,kind,id}`). The account's linked originals are
  listed by name. Reconnecting issues a **new device capability** that the browser
  stores like a guest capability. The original capability and earlier devices keep
  working. Each original keeps at most 16 device capabilities, dropping the oldest
  first.
- **Recovery.** The account service keeps the `account_id` across passkey loss
  (an email magic link starts a new session for the same account). Linked libraries
  therefore stay linked and can be reconnected after recovery.
- **Deletion** by any owner device removes the original for every device and from
  the account listing. There is no unlink yet.

`kind` is `wordList`, `tileProfile` or `layout`. Every account route is a JSON
`POST` with `version: 1`. It requires the board's own `Origin` (cookie-carrying
calls from anywhere else are refused) and answers `401 signed-out` with `signIn`
(the account page) when there is no valid session.

## Operation

`SCRABBLE_ACCOUNTS_URL` is the account service's public base URL, for example
`https://moenarch.com/accounts/`; its routes resolve below it. Linking is off when it is unset. Production requires
HTTPS; local mode also accepts `http://` for test doubles. Linked owners and device
verifiers live in the existing library files (`accountId`, `deviceVerifiers`). Files
written before #132 load unchanged.

The browser sends the account cookie only when the page and API are on the same
site as the cookie domain (`moenarch.com`). The account-enabled client is therefore
served at `https://moenarch.com/scrabble/` with its API at
`https://moenarch.com/scrabble-api`, next to the account service at `/accounts/` (#206). The GitHub Pages build is a guest-only
demo: its origin is listed in `SCRABBLE_GUEST_BOARD_ORIGINS`, which admits the game
and library routes but answers every account request with `403 guest-origin`. The
client treats that like a server without accounts and hides the account section.

## Tests

`crates/scrabble-server/tests/account_linking.rs` runs the hosted server against a
fake account service that implements the verification contract. It covers all three
kinds, a missing capability, signed out, duplicate link, wrong owner, a second device,
recovery after restart, deletion and an unreachable service.
`apps/web/src/account-link.test.ts` covers the browser client. Live cross-device
acceptance against the deployed account service is **unverified**.
