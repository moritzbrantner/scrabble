# LAN real-device acceptance (#32)

The owner runs this by hand. You need one desktop or laptop for the shared board and at least two physical phones on the same LAN. It exercises the local authoritative server (`bun run dev:local`) and needs no public hosting; that is #67. Rerun it unchanged after each release. It takes about 30 minutes.

## Devices and browsers

| Role                | Device                       | Browser                                                     |
| ------------------- | ---------------------------- | ----------------------------------------------------------- |
| Host + shared board | Linux desktop/laptop         | Chrome or Edge (WebTransport with certificate hashes)       |
| Phone A             | Android phone                | Chrome for Android                                          |
| Phone B             | Second phone (Android / iOS) | Chrome for Android, or Safari on iOS (record which browser) |

Nothing is installed on the phones. They open the invite in their browser. A phone that cannot connect counts as a failure. Record its browser and version in the result table.

## 1. Prepare the host (once per checkout)

From a fresh clone of `moritzbrantner/scrabble` on the host:

```sh
git pull --ff-only && git rev-parse --short HEAD          # record this SHA
bun install --frozen-lockfile
# Dictionary (skip if .local/deployment/content/provenance.json already exists):
mkdir -p .local/deployment
git clone --depth 1 --branch rel-2026.02.25 https://github.com/en-wl/wordlist.git .local/scowl-source
bun scripts/prepare-dictionary.ts .local/scowl-source .local/deployment/content
```

The host needs Docker Engine with Compose on the default local context, and Make, Python 3 and SQLite for the dictionary step.

## 2. Network prerequisites

- All devices are on the same Wi-Fi/Ethernet network. Wi-Fi **client isolation** (AP isolation or a guest network) is off.
- The host firewall allows **TCP 5173** (HTTPS client + `/api`) and **UDP 4433** (WebTransport) from the LAN. With ufw, for example: `sudo ufw allow 5173/tcp && sudo ufw allow 4433/udp`.
- Note the host's LAN IPv4 (`ip -4 addr`). If there is more than one interface, pin it with `LOCAL_LAN_IP=<ip>` below.
- Phones must not use a VPN or private relay that bypasses the LAN.

## 3. Start the server

```sh
mkdir -p test-results/lan-acceptance
LOCAL_LAN_IP=<ip> bun run dev:local 2>&1 | tee test-results/lan-acceptance/launcher.log
```

The launcher prints `Open this local game URL: https://<ip>:5173/scrabble/?server=…&api=…&certificate=…`. It also saves that URL to `.local/local-server/url.txt`.

## 4. Run the acceptance steps

Take a screenshot at every step marked **[S]**. Name it `NN-device.png` (for example `05-phoneA.png`) and save it in `test-results/lan-acceptance/`.

| #   | Step                                                                                                                                                               | Expected result                                                                                                                                                                         | Criterion |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- |
| 1   | On the board browser, open the **complete** printed URL and accept the self-signed certificate warning. **[S]**                                                    | The create-game screen loads with no "service unavailable" message.                                                                                                                     | AC1, AC5  |
| 2   | Click **Create game** with the default settings. **[S]**                                                                                                           | The lobby shows a QR code and a player link. The status line reports connected. Record the match ID from the link (`match=`).                                                           | AC1, AC4  |
| 3   | On phone A, scan the QR code with the camera. Accept the certificate warning, enter the name "A" and join. **[S]**                                                 | The phone shows "Player 1" (or similar) with a private rack. The board roster lists A.                                                                                                  | AC2       |
| 4   | Repeat step 3 on phone B with the name "B". **[S]**                                                                                                                | B gets a different player ID. The board lists A and B. Neither phone shows the other's tiles.                                                                                           | AC2, AC3  |
| 5   | On the board, click **Start game**. **[S]** board + both phones                                                                                                    | The invite QR disappears. Both phones show a 7-tile rack and whose turn it is. The board shows only rack _counts_, never letters.                                                       | AC2       |
| 6   | The active phone places a word across the centre (premium) square but does **not** commit. **[S]** active phone + board                                            | The active phone shows its tentative placement and score preview. Per Decision 2.6 the **board and the other phone show nothing** until the commit.                                     | AC2       |
| 7   | Click **Commit move**. **[S]** board                                                                                                                               | The board shows the committed tiles. The score includes the premium. The turn passes to the other player and the turn history lists the word. The committing phone's rack refills to 7. | AC2       |
| 8   | Play at least two more committed words, each **crossing** an existing word. Use a blank if anyone draws one. **[S]** after each                                    | Every commit shows the same board, scores and turn on all three screens. Rejected words show an error on the phone and change nothing.                                                  | AC1, AC2  |
| 9   | Reconnect: on the phone whose turn it is **not**, note its player ID and rack. Pull to refresh (or reload). Then turn its Wi-Fi off for ~10 s and back on. **[S]** | The status goes through reconnecting or restoring to connected. The player ID and rack are unchanged. No new roster seat appears and no other player's tiles are shown.                 | AC3       |
| 10  | Reconnect the active phone mid-draft: place a tile, reload, then finish and commit a move.                                                                         | After the reload the draft is cleared (by design). The same identity and rack come back and the commit is accepted.                                                                     | AC3       |
| 11  | Refresh the board browser.                                                                                                                                         | The board resumes the same game, board, scores and history. No new game is created.                                                                                                     | AC1, AC3  |
| 12  | Exchange: the active player clicks **Exchange tiles** and swaps at least one tile. **[S]**                                                                         | History shows "Exchanged N tiles". That phone's rack changes and the other phone and the board see no letters.                                                                          | AC2       |
| 13  | Finish: alternate **Pass turn** / **Exchange tiles** until six scoreless turns in a row end the game (English ruleset). **[S]** board + both phones                | The board shows **Game finished** with final scores after rack deductions and the winner or tie. The phones show read-only racks and final scores.                                      | AC1, AC2  |
| 14  | Throughout, confirm you never touched server state or files.                                                                                                       | No manual server-side editing was needed.                                                                                                                                               | AC4       |

## 5. Capture evidence

Do this after step 13, before you stop the launcher:

```sh
docker logs --timestamps scrabble-local-server-1 > test-results/lan-acceptance/server.log 2>&1
git rev-parse HEAD > test-results/lan-acceptance/commit.txt
```

Then press Ctrl+C in the launcher terminal. It stops the server gracefully and writes the recovery image. Keep a **private** copy of it for replaying failures:

```sh
cp -a .local/local-server/state/recovery test-results/lan-acceptance/recovery-private
```

The recovery image and `url.txt` hold private capabilities. Do not attach them to GitHub. Attach the screenshots, `server.log` and `launcher.log`.

If a phone fails, capture its browser console before you reload. On Android, connect USB and open `chrome://inspect` on the host. On iOS, use Safari > Develop > [device]. Record the visible status text: `connecting`, `reconnecting (attempt N)`, `restoring`, `failed`, or `resume stopped`.

## 6. Results

Copy this table into a comment on #32. Any **Fail** keeps #32 open. Add a linked bug with the step number, screenshots and log excerpt.

```md
LAN real-device acceptance — commit `<sha>` — <date>
Host: <OS>, board browser <name/version>. Phone A: <model>, <browser/version>. Phone B: <model>, <browser/version>.
Match ID: <id>

| #   | Step                          | Pass/Fail | Notes |
| --- | ----------------------------- | --------- | ----- |
| 1   | Board opens printed URL       |           |       |
| 2   | Create game, QR shown         |           |       |
| 3   | Phone A joins via QR          |           |       |
| 4   | Phone B joins, distinct ID    |           |       |
| 5   | Start, private racks          |           |       |
| 6   | Tentative preview private     |           |       |
| 7   | Premium commit, turn advances |           |       |
| 8   | ≥2 crossing commits coherent  |           |       |
| 9   | Idle phone refresh/Wi-Fi drop |           |       |
| 10  | Active phone reload + commit  |           |       |
| 11  | Board refresh                 |           |       |
| 12  | Exchange                      |           |       |
| 13  | Game finishes, final scores   |           |       |
| 14  | No manual server-side edits   |           |       |

Blank played: yes/no. Evidence attached: screenshots / server.log / launcher.log.
```
