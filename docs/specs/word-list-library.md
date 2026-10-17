# Guest custom word-list library

Issue #97, guest-owned slice. Atlas Decisions 1.1, 1.8, 1.16, 1.18, 1.20, 1.25
and 1.45; ADR 0008/0009. It mirrors the custom board-layout library (#101).
Optional account linking (Decisions 1.20/1.46, Question 1.44) is deferred by
Decision 1.45 to #132 and is not part of this contract.

Acceptance: `crates/scrabble-server/tests/custom_word_lists.rs` (native host,
HTTP API, configured factory, recovery and native replay) and
`apps/web/browser/custom-word-lists.spec.ts` (Chromium against the native
fixture, in the dev and Pages suites).

## Ownership and storage

- A guest creates a list without an account. The server returns a random
  64-hex **owner capability** only to its creator. The browser keeps it in
  `localStorage` under `scrabble:word-list-owner:v1:<wordListId>`, bound to the
  game API that issued it. It never appears in a URL, the DOM, session storage,
  a share link or any public projection.
- `SCRABBLE_WORD_LIST_DIR` (optional; default `word-lists/` beside the seed;
  absolute in production and separate from TLS, seed, recovery, match
  initialization and board layouts) holds one private `w_<id>.json` entry per
  original and one `w_<id>-r<revision>.txt` content file per retained version.
  The entry stores the name, an optional share id, version metadata and an HMAC
  verifier of the capability keyed from the private seed — never the capability.
  Content files hold the normalized word set (sorted distinct words, one per
  line) and are verified against their SHA-256 on every read.
- The library cannot be used for owner management if the seed is replaced, so
  the server refuses to create a new seed while the directory is non-empty.
  Losing the browser's capability loses management; no recovery is offered and
  the interface says so (Decision 1.45).

## Text format and bounds

The text is exactly the deployment dictionary format
(`docs/specs/dictionary-validation.md`), parsed by the same `WordList` parser
and fingerprinted by the same `WordListProfile`: one uppercase alphabetic word
per line in any Unicode alphabet, LF or CRLF, blank lines ignored, duplicates
and order irrelevant. Lowercase, spaces, digits, punctuation and comments are
rejected with the 1-based line number. The browser only decodes the chosen file
strictly as UTF-8 (rejecting undecodable bytes, empty and oversize files before
sending) and lets the player review the text; the server decides validity.

`GET /word-lists/limits` publishes the bounds: at most **1 MiB** (1,048,576
UTF-8 bytes) and **200,000** distinct words per list. The server additionally
keeps at most 1,024 originals, 8 selectable versions per original and 256 MiB
of retained word content in total (`429 word-list-library-full` beyond). Only
the word-list routes accept request bodies up to 2 × 1 MiB + 4 KiB (JSON
escaping of line breaks); every other route keeps its existing cap.

## HTTP API

JSON with `version: 1`; every mutation is a POST.

| Route                                    | Request                                | Response                                                                  |
| ---------------------------------------- | -------------------------------------- | ------------------------------------------------------------------------- |
| `GET /word-lists/limits`                 | —                                      | `{version,maxBytes,maxWords}`                                             |
| `POST /word-lists`                       | `{version,name,words}`                 | `{version,wordListId,ownerCapability,name,revision:1,wordList,wordCount}` |
| `POST /word-lists/{id}/read`             | `{version,ownerCapability}`            | `{version,wordListId,name,revision,wordList,wordCount,words,shareId?}`    |
| `POST /word-lists/{id}/update`           | `{version,ownerCapability,name,words}` | read shape, `revision+1`, new `wordList`                                  |
| `POST /word-lists/{id}/delete`           | `{version,ownerCapability}`            | 204                                                                       |
| `POST /word-lists/{id}/share`            | `{version,ownerCapability}`            | `{version,shareId}` (idempotent)                                          |
| `POST /word-lists/{id}/unshare`          | `{version,ownerCapability}`            | 204 (idempotent)                                                          |
| `GET /shared-word-lists/{shareId}`       | —                                      | `{version,shareId,name,revision,wordList,wordCount}`                      |
| `POST /shared-word-lists/{shareId}/copy` | `{version,name}`                       | create shape for a new, independently owned original                      |

`wordListId` is `w_<32 hex>`, `shareId` is `s_<32 hex>`. `wordList` is the
version's `ContentIdentity`: a random per-version source name plus the
content-hash revision used for every word-list profile. The shared view always
shows the original's current version and never its id or capability. Names are
1–64 characters without surrounding spaces or control characters. A copy starts
unshared.

Errors: `400 invalid-word-list` with `reason` and, when one line is at fault,
`line`; `413 word-list-too-large` with `reason`, `maxBytes`, `maxWords` (or the
framing-level `413 body-too-large`); `403 word-list-owner-rejected` for a
missing, forged or foreign capability; `404 unknown-word-list` for unknown,
deleted or withdrawn lists and shares; malformed bodies are `400
invalid-request`.

## Match creation and immutability

Version-3 `POST /games` accepts the `wordList` identity of a retained saved
version or of a share view as `selection.wordList`, while tiles and board are
chosen independently (Decision 1.25). The factory resolves each component from
the operator catalog first, then from the guest libraries, and composes them
through the single existing compatibility check: an incompatible pair is
`incompatible-profiles` with the missing letters, never a substitution. Forged,
inline, dropped or deleted identities are `unknown-profile/wordList`.

The configured initialization manifest and the content-addressed word store
(ADR 0009) pin the selected word set. Started matches never read the library
again, so editing, unsharing or deleting a list, a dropped version or a server
restart never changes their validation, recovery or native replay.

Two choices are not settled by the Atlas and follow the board-layout library:

- After unsharing, an identity already learned from the share view remains
  selectable for new matches while that version is retained; withdrawal stops
  the share view and new copies, not identity resolution (as for layouts, whose
  board identities resolve independently of sharing).
- Sharing again after a withdrawal issues a new share id; a withdrawn id is
  never revived.

## Browser

The create page has a "Custom word lists" region: "New word list", the
"Saved word lists" list (name, word count, Edit/Share/Stop sharing/Delete) and
the honest notice. The "Word list editor" region has the "Word-list file"
input, "Word list name" and "Words" fields and "Save word list"; problems are a
single alert (the server's line number, UTF-8, empty or size feedback), and a
failed save stores nothing. "Share" shows the "Word list share link"
(`view=word-list&share=…` plus dev-only connection parameters). The share view
"Shared word list" offers "Save a copy" and "Use for a new game"; a withdrawn
link shows an alert. Saved and opened shared lists are options of the existing
"Word list" selector; choosing one never changes the tiles, board or interface
language. All strings exist in English, German and Spanish.
