# Production deployment

The authoritative service runs separately from GitHub Pages. The committed Compose topology exposes UDP 443 for native WebTransport and TCP 443 for HTTPS creation/invite lookup. Its gateway exposes only `/api/`, `/healthz`, and `/readyz`; detailed runtime status and management endpoints stay inside the Compose network. Both services run without root privileges or writable image filesystems.

The public target is the existing Hetzner host at `moenarch.com`. The deployment below runs the same executable behind the host nginx service. The Compose topology remains the canonical isolated deployment check and an option for dedicated hosts. Local hosting remains available; see [local game setup](server-executable.md#local-game-setup). Physical phone acceptance is separate from desktop browser acceptance.

## Existing Hetzner host

The host has systemd and nginx, with other applications already using TCP 443. Run the binary built by the pinned Dockerfile under `deploy/scrabble-server.service`, rather than installing a second container gateway. This deployment adapter deliberately reuses existing host infrastructure; it does not replace the canonical Compose test topology.

- Install the executable at `/opt/scrabble/scrabble-server`, owned by root, mode 0755. Build it from the reviewed revision with the repository Dockerfile, extract `/usr/local/bin/scrabble-server`, and record its SHA-256. Do not deploy a binary built against a newer host glibc.
- Create the `scrabble` system account and install `deploy/hetzner.env.example` as `/etc/scrabble/server.env`, mode 0600. The unit creates owner-only `/var/lib/scrabble` for the seed and recovery bundle. Retain the SCOWL content and notices at `/opt/scrabble/content`.
- The unit uses systemd credentials to read the existing moenarch.com certificate and key without granting the service access to the certificate directory. Install `deploy/renew-scrabble-tls` as an executable Certbot deploy hook; it gracefully restarts only after that certificate is renewed.
- Include `deploy/hetzner-nginx.conf` inside the existing moenarch.com TLS server block. Run `nginx -t` before reloading. HTTPS creation and lookup use `/scrabble-api/`; health and readiness use `/scrabble-healthz` and `/scrabble-readyz`. Detailed management routes are not proxied, and request access logging is disabled for these locations.
- Permit UDP 443 through the host and provider firewalls for WebTransport. Keep TCP 18080 and 18081 blocked from the public network: the native executable binds those listeners to all interfaces, so host firewall isolation is required.
- Enable and start `scrabble-server.service`, check readiness through trusted HTTPS, and then release Pages with the committed `deploy/client.json`. The game endpoint is `https://moenarch.com/game`; the API endpoint is `https://moenarch.com/scrabble-api`. TCP 443 continues to terminate at nginx, while UDP 443 terminates at Scrabble. Do not advertise HTTP/3 on this UDP port.

For upgrades, stop the unit gracefully, back up `/var/lib/scrabble` with owner-only permissions, retain the previous executable and immutable content, install the candidate, and start the unit. Apply the same recovery/rollback restrictions described below. Certificate-hook restarts use the same graceful shutdown and recovery boundary.

## Host prerequisites

Use a host with Docker Compose, a stable public address, and inbound TCP and UDP 443. Point the chosen service domain at that host. Supply a publicly trusted certificate chain as `cert.pem` and its private key as `key.pem`; the certificate must cover the service domain. The UDP listener terminates TLS itself, so an HTTP-only reverse proxy does not provide the game transport. Do not advertise HTTP/3 for the HTTPS API on the game's UDP port.

Create three host directories outside disposable containers: TLS, persistent state, and dictionary content. Copy `.env.example` to an uncommitted deployment environment file and set the `DEPLOY_*` paths to absolute host paths. Set the numeric UID/GID to the owner of these directories. Restrict private key and state access to that owner. The state directory must be writable by the server; TLS and content are mounted read-only. Initial startup creates the private initialization seed in the state directory.

Place the chosen word list in `words.txt` and explicitly set its dictionary name and revision. Retain source, license notices, and preparation instructions alongside it. The authored fixture is suitable for smoke tests only; it is not a complete production dictionary. To select a custom ruleset, add `-f compose.ruleset.yaml` after `-f compose.yaml`, place `ruleset.json` in the content directory, and use the dictionary identity declared by that profile. See [configurable rulesets](configurable-rulesets.md).

The browser origin is `https://moritzbrantner.github.io`, without `/scrabble/`. Set `DEPLOY_BOARD_ORIGIN` to the exact origin if Pages moves. Production rejects missing or different WebTransport origins before admission and requires the declared origin on creation requests. Origin checking is a browser boundary; it does not authenticate native clients.

## Startup and public configuration

### Reproducible initial English dictionary

The optional initial corpus uses SCOWL's general American English spelling vocabulary, size 60 and variant level 1. This is not an official tournament Scrabble dictionary. The preparation excludes abbreviations and special categories, then keeps only lowercase ASCII words of length 2–15, uppercases them, and sorts unique entries. This excludes capitalized names, punctuation, accents, and longer words. It produces 78,026 words with SHA-256 `3e8c1a3f17f6d1a2fbad509b4454908d5edb9bf86e8ab8595a029e14418c0ad6`.

Install Git, Make, Python 3, and SQLite according to the [upstream prerequisites](https://github.com/en-wl/wordlist/blob/7e99edab8e32f9f9ea2b15f249ca8d4d67237410/README.md). In an operator-owned preparation directory:

```sh
git clone --depth 1 --branch rel-2026.02.25 https://github.com/en-wl/wordlist.git scowl-source
bun scripts/prepare-dictionary.ts scowl-source /ABSOLUTE/NEW/CONTENT_DIRECTORY
```

The script verifies the exact release commit and unmodified tracked source, rebuilds the source database, and checks the output count and hash before creating the new content directory. It refuses an existing destination. It retains the complete upstream copyright notice, source README, and a provenance manifest alongside `words.txt`. The [upstream notice](https://github.com/en-wl/wordlist/blob/7e99edab8e32f9f9ea2b15f249ca8d4d67237410/Copyright) specifies redistribution conditions; preserve these artifacts when copying the deployment content. No dictionary download or preparation runs in the serving process or Pages build.

For this corpus set `DEPLOY_DICTIONARY_NAME=scowl-en-us-60`, `DEPLOY_DICTIONARY_REVISION=2026.02.25-recipe1`, and `DEPLOY_CONTENT_DIRECTORY` to the prepared directory. Keep the original content immutable for recovery and rollback. The runtime additionally fingerprints the semantic word set and ruleset; the provenance hash records the prepared file bytes.

Run `bun run test:deployment /ABSOLUTE/NEW/CONTENT_DIRECTORY` to check the prepared corpus in the real container topology. This checks the provenance file hash before loading the corpus, then exercises HTTPS creation, UDP snapshots, and graceful recovery with that dictionary identity. Without this argument the smoke test uses the authored fixture.

### Word-list presets

The public create screen offers English (US and UK), expanded English (ENABLE2K), German and Spanish word lists (#153), each labelled with its source and edition and identified as non-official tournament vocabulary, plus German (102 tiles) and Spanish (100 tiles with CH, LL and RR) tile profiles. Word list, tiles and board stay independently selectable; the server rejects incompatible pairs. Official tournament lexicons are not offered. The original SCOWL US list remains the built-in default; ENABLE2K is an additional selection for new matches only.

The lists are generated by the operator at deploy time from pinned free sources and are never committed:

| List               | Source (pinned)                                                      | Licence                      |   Words |
| ------------------ | -------------------------------------------------------------------- | ---------------------------- | ------: |
| English (expanded) | ENABLE2K via BartMassey/wordlists (`af52415`), 2–15 letters          | Public domain                | 169,266 |
| English (UK)       | SCOWL `rel-2026.02.25` (`7e99eda`), size 60, British `-ise` spelling | SCOWL notice                 |  78,021 |
| Deutsch            | igerman98 via LibreOffice dictionaries `de/de_DE_frami` (`67032ee`)  | GPL-2.0 or GPL-3.0           | 540,287 |
| Español (España)   | RLA-ES 2.9 via LibreOffice dictionaries `es/es_ES` (`67032ee`)       | GPL-3.0, LGPL-3.0 or MPL-1.1 | 580,715 |

German and Spanish forms come from expanding the Hunspell affix rules (`scripts/hunspell.ts`); compound words are not generated, so only listed and inflected forms are included. German keeps Ä, Ö and Ü as letters and writes ß as SS. Spanish drops accents and diaeresis except Ñ and keeps only words spellable with Spanish tiles (no K or W); CH, LL and RR are spelled with their letters and the tiles add two-letter faces. Abbreviations are dropped in both. Spanish proper names are dropped because they are capitalized; igerman98 does not mark proper names and German nouns are capitalized, so the German list can contain some names.

In an operator-owned preparation directory, with the SCOWL prerequisites above:

```sh
git clone --depth 1 --branch rel-2026.02.25 https://github.com/en-wl/wordlist.git scowl-source
git clone --filter=blob:none --no-checkout https://github.com/LibreOffice/dictionaries.git lo-dictionaries
git -C lo-dictionaries sparse-checkout set --no-cone /de /es
git -C lo-dictionaries checkout 67032ee957631908098a72edf48dc81cac27e369
git clone https://github.com/BartMassey/wordlists.git enable2k-source
git -C enable2k-source checkout af52415c13af809bd8757a40f17f46e79d09583c
bun scripts/prepare-word-lists.ts scowl-source lo-dictionaries enable2k-source /opt/scrabble/presets
```

The script checks all three source revisions and unmodified sources, verifies the ENABLE2K archive checksum, and verifies each output's reviewed count and SHA-256 before creating the new directory; it refuses an existing one. It reports additions and omissions relative to the current SCOWL US recipe, including short-word coverage. The directory holds `catalog.json` (an `includeBuiltin` catalog, so the built-in en-US list, tiles, boards and legacy default stay unchanged), the four word files, every licence and source notice, and `provenance.json`. Make it readable by the `scrabble` service account (for example `chown -R root:scrabble` and `chmod -R g+rX`), and keep it immutable like the dictionary content: started matches pin their word set. Set `SCRABBLE_PROFILE_CATALOG_FILE=/opt/scrabble/presets/catalog.json` (already in `deploy/hetzner.env.example`); the server refuses to start if that file is missing.

ENABLE2K is Alan Beale and M. Cooper's broad, unofficial millennial word-game list, not a current tournament lexicon or a strict superset of SCOWL. Its 173,528 source entries yield 169,266 unique uppercase ASCII words after the existing 2–15 letter restriction. Keep `ENABLE2K-README.txt` intact: it contains the original public-domain release, incorporation permission and originator credit. Provenance also records the exact compressed source filename and SHA-256. Existing deployments must regenerate the preset directory into a **new** immutable location and switch the catalog path to offer this additional choice; existing catalogs are not modified by installing the executable.

To verify the actual prepared corpora, place the outputs of `prepare-dictionary.ts` and `prepare-word-lists.ts` under `baseline/` and `presets/` in an acceptance directory, respectively, then run:

```sh
export SCRABBLE_PRESET_ACCEPTANCE_ROOT=/ABSOLUTE/ACCEPTANCE_DIRECTORY
cargo test -p scrabble-server --test word_list_presets prepared_enable2k --locked -- --ignored
bun scripts/test-browser.ts word-list-presets.spec.ts
bun scripts/test-browser.ts --pages word-list-presets.spec.ts
```

The native acceptance checks real catalog creation/retry, unknown-revision rejection, valid and invalid words, restart, immutable configuration, recovery and canonical replay after removing ENABLE2K from future catalog choices. Chromium acceptance checks unchanged SCOWL US defaults, retained UK selection, independent ENABLE2K selection, desktop/mobile setup and match reload against the real server. Screenshots are written to `test-results/`. These corpus checks are opt-in because third-party generated lists are not committed or downloaded by the hermetic gate; the ordinary gate still checks normalization, source substitution rejection and catalog behavior. Unset `SCRABBLE_PRESET_ACCEPTANCE_ROOT` before running the ordinary browser suites, which use authored dictionaries.

### Deploying the service

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

The repository's native process tests prove graceful recovery, private projections, and reconnect fencing. Browser checks prove real loopback WebTransport gameplay and production configuration failure behavior. The container smoke test does not prove public certificate trust or gameplay through a public deployed host. Publicly trusted Pages transport, deployed health probes, and rollout compatibility remain future public-hosting acceptance; physical phone gameplay remains separate from same-computer browser evidence.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
