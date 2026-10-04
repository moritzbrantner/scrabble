# Configurable rulesets and dictionaries

`Ruleset` owns versioned identity, dictionary identity, tile frequencies/values, board dimensions/premiums, rack size, bingo bonus, exchange threshold, scoreless limit, and player limits. The deterministic placement, scoring, turn, blank, and completion engine consumes that data without selecting a language-specific branch. Tile faces are individual alphabetic Unicode scalars; blank assignment must belong to the configured set.

The default remains the English authored fixture. To configure another profile, set `SCRABBLE_RULESET_FILE` to a regular UTF-8 JSON file, together with the three dictionary variables. The JSON structure is demonstrated by [the authored German mini profile](../../crates/scrabble-game/fixtures/de-mini-ruleset.json). Its deliberately small 7×7 board, three-tile racks, bonus of 20, umlauts, and distinct frequencies/values demonstrate the boundary; it is not an official German distribution or a tournament word list. [Its small word list](../../crates/scrabble-game/fixtures/de-words.txt) is authored test data.

For example, with absolute deployment paths:

```sh
SCRABBLE_RULESET_FILE=/var/lib/scrabble/de-mini-ruleset.json
SCRABBLE_DICTIONARY_FILE=/var/lib/scrabble/de-words.txt
SCRABBLE_DICTIONARY_NAME=scrabble-de-authored-fixture
SCRABBLE_DICTIONARY_REVISION=1
```

The JSON dictionary name/revision must match those explicit variables before the loader adds content fingerprints. Custom profiles never fall back to the default word list. Operators supply lists they are entitled to use; no third-party dictionary is bundled or downloaded. The configured words must use the configured alphabet and the word-list format described in [dictionary validation](dictionary-validation.md). Interface language preferences remain independent of this server-selected match profile.

## Validation and bounds

Loading and validation finish before private seed initialization or listener startup. Unknown/missing JSON fields, invalid dimensions/player limits, duplicate faces, nonzero blank values, empty frequencies, insufficient tiles, unsupported dictionary letters, and mismatched dictionary identities fail closed. Ruleset files are bounded to 128 KiB; dictionary files remain bounded to 64 MiB. Production requires absolute paths.

Profiles support odd boards of at most 15×15, rack sizes of 1–15, 2–4 players, at most 64 distinct tile definitions including the optional blank, and at most 200 total tiles. Content names are ASCII identifiers up to 64 bytes; revisions are bounded to 192 bytes, with custom base revisions up to 64 bytes. The limits keep full public projections and private replay checkpoints within the native 65,535-byte snapshot contract, including bounded maximum history and four-byte letters. Values and bonuses remain bounded integers; score arithmetic remains checked atomically.

## Immutable identity and presentation

The loader normalizes dictionary order/duplicates/line endings before computing its word-set digest. A custom profile receives `<base revision>+rules-sha256:<digest>` over the full canonical ruleset data and fingerprinted dictionary identity. JSON object field order/whitespace do not change this identity; tile/premium order and every semantic setting do. The factory shares an immutable validated profile and dictionary. Editing deployment files does not alter existing factories or started matches.

Canonical game evidence already contains the complete ruleset. Native replay/recovery verifies initialization against that evidence and rejects changed profiles, even when their nominal name/base revision is unchanged. Restore original configuration files to replay or recover existing matches; no implicit migration occurs.

Public snapshots now require `configuration`, containing the immutable public ruleset data. Dictionary words, initialization keys, bag order, and other racks remain private. The browser validates the configuration and its declared identities, derives dimensions/letters/values/blank choices from it, and rejects any change during the connection. It no longer substitutes English geometry. The frontend and native server must be released together for this required public DTO field; native wire protocol 3, command version 1, and canonical game evidence version 3 remain unchanged.

## Evidence

Native tests accept and score the configured Unicode blank (ÖL), apply the configured three-tile bingo (BÄR), retain the original profile after file changes, preserve state/replay on unsupported blank rejection, and reject replay/recovery after changes to values, frequencies, rack size, bonus, premiums, or turn limits. Invalid/incomplete/oversized profiles fail before seed creation. Worst-case public and private checkpoint serialization tests exercise the native payload limit.

Rust generates the browser variant fixture from the actual accepted blank commit. Browser/unit checks derive 7×7 geometry, a three-tile rack, L's configured value, and the configured Unicode choices, then reconcile the authoritative commit. The complete default-English native browser and built Pages loops continue to exercise the same engine and transport path. Run `bun run check`.

Shared convention source revision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
