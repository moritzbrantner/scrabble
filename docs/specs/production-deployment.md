# Production deployment

The authoritative service runs separately from GitHub Pages. The committed Compose topology exposes UDP 443 for native WebTransport and TCP 443 for HTTPS creation/invite lookup. Its gateway exposes only `/api/`, `/healthz`, and `/readyz`; detailed runtime status and management endpoints stay inside the Compose network. Both services run without root privileges or writable image filesystems.

The hosting provider and public service domain have not been supplied. This document describes the prepared topology, not evidence of a deployed service. Physical phone acceptance is also outstanding.

## Host prerequisites

Use a host with Docker Compose, a stable public address, and inbound TCP and UDP 443. Point the chosen service domain at that host. Supply a publicly trusted certificate chain as `cert.pem` and its private key as `key.pem`; the certificate must cover the service domain. The UDP listener terminates TLS itself, so an HTTP-only reverse proxy does not provide the game transport. Do not advertise HTTP/3 for the HTTPS API on the game's UDP port.

Create three host directories outside disposable containers: TLS, persistent state, and dictionary content. Copy `.env.example` to an uncommitted deployment environment file and set the `DEPLOY_*` paths to absolute host paths. Set the numeric UID/GID to the owner of these directories. Restrict private key and state access to that owner. The state directory must be writable by the server; TLS and content are mounted read-only. Initial startup creates the private initialization seed in the state directory.

Place the chosen word list in `words.txt` and explicitly set its dictionary name and revision. Retain source, license notices, and preparation instructions alongside it. The authored fixture is suitable for smoke tests only; it is not a complete production dictionary. To select a custom ruleset, add `-f compose.ruleset.yaml` after `-f compose.yaml`, place `ruleset.json` in the content directory, and use the dictionary identity declared by that profile. See [configurable rulesets](configurable-rulesets.md).

The browser origin is `https://moritzbrantner.github.io`, without `/scrabble/`. Set `DEPLOY_BOARD_ORIGIN` to the exact origin if Pages moves. Production rejects missing or different WebTransport origins before admission and requires the declared origin on creation requests. Origin checking is a browser boundary; it does not authenticate native clients.

## Startup and public configuration

From the reviewed checkout, using the deployment environment file:

```sh
docker compose --env-file .env --profile production config --quiet
docker compose --env-file .env --profile production build server
docker compose --env-file .env --profile production up -d --wait
```

Set `DEPLOY_VERSION` to the deployed revision before building. Record the resulting server image digest and preserve the previous image for rollback. The Dockerfile pins its builder and runtime base images by digest; the gateway is also digest-pinned.

Once the service is reachable with normal TLS trust, set the two public fields in `deploy/client.json` to `https://SERVICE_DOMAIN/game` and `https://SERVICE_DOMAIN/api`. Commit and review these routing settings. The Pages workflow runs `scripts/build-pages.ts`, which validates them and builds the checked artifact. Neither field accepts credentials or private query parameters. Private TLS keys, seed bytes, dictionary files, and reconnect capabilities must never enter this file or `VITE_*` settings.

An empty public configuration builds an explicit service-unavailable screen. A malformed configuration fails the release build. Development fixture routes and query endpoint overrides do not activate in a release build. Browser tests check these behaviors against compiled production assets and verify that a test certificate canary is absent. Release connections use normal browser certificate trust.

## Health and rollout

Check `https://SERVICE_DOMAIN/healthz` and `/readyz` independently of gameplay. Readiness becomes unavailable during draining. Internal detailed status is available to an operator through the private Compose network; do not publish the server's management TCP port.

Before an upgrade, retain the running image, original dictionary/ruleset, TLS identity, and deployment settings. Stop the gateway, then stop the server gracefully:

```sh
docker compose --env-file .env --profile production stop gateway server
```

After both services stop, back up the entire state directory, including the seed and recovery bundle, with owner-only access. A hard kill does not create a fresh recovery bundle. Never copy a live directory as if it were a consistent checkpoint.

Build the candidate server, then recreate both services so the gateway resolves the recreated server's network address:

```sh
docker compose --env-file .env --profile production build server
docker compose --env-file .env --profile production up -d --wait --force-recreate server gateway
```

Recovery verifies the saved ruleset/dictionary identities and restores dynamically created matches, reconnect capabilities, and sequence cursors. The private seed remains stable. A successful startup consumes the recovery bundle, so retain the stopped-state backup before attempting an upgrade. Never delete the seed to bypass a recovery error. Existing browser sessions can reconnect within the configured one-hour grace window; a longer outage can expire their seats. Clients report disconnects and incompatible snapshots explicitly.

For a failed upgrade that has accepted no new gameplay, stop both services and restore the stopped-state backup, original content, and previous image before restarting. Restoring that backup after new gameplay has been accepted would discard those actions; do not treat it as a transparent rollback. Recovery and wire compatibility must be checked for each release before rollout. See [server recovery](server-executable.md).

## Verification boundaries

`bun run test:deployment` builds and starts the canonical production topology on loopback ports using an owned temporary project, test certificate, fixture dictionary, and private state. It verifies HTTPS health, API origin rejection, hidden management routes, actual UDP WebTransport admission and snapshot identity, rejection of missing/wrong transport origins before seat allocation, and graceful restart preserving match and seed identity, then removes its containers and temporary files. It requires a local Unix Docker daemon and uses curl certificate verification against the generated test certificate.

The repository's native process tests prove graceful recovery, private projections, and reconnect fencing. Browser checks prove real loopback WebTransport gameplay and production configuration failure behavior. The container smoke test does not prove public certificate trust or gameplay through the public deployed host. Publicly trusted transport from the deployed Pages URL, real public health probes, and physical phone gameplay must still be recorded against the actual deployment before #31 and #32 can be closed.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
