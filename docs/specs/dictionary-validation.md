# Immutable dictionary validation

`Dictionary` supplies an explicit content identity and a deterministic, immutable local lookup. `GameSession` retains the selected dictionary behind an `Arc`; commits first validate structure, then check every distinct main/cross word in sorted order. The dictionary identity must exactly match the ruleset. The default `WordList` is a sorted unique set; source ordering, duplicate entries and CRLF versus LF do not affect lookup or deployment fingerprints. The small committed fixture is repository-authored and needs no external service.

Invalid moves return `DictionaryError::InvalidWords { words }`, a serializable `invalid_words` result containing every rejected word in stable order. No rack, bag, board, score, turn, preview or sequence changes. The simulation wraps only invalid-word failures in a bounded private response carrying the game ID and expected turn. `game-server` sends the attempted sequence through its reliable rejection stream only to that authenticated connection. The phone validates the response and matches the submitted sequence/game/turn before displaying the words, retaining the draft and allowing correction. Rejection is never command acceptance: watermark/replay admission still fails. Ordinary protocol, ownership and authority failures retain fatal closure. Private rejection bytes are absent from transport diagnostics and public snapshots.

## Deployment configuration

Set all three server variables together:

```sh
SCRABBLE_DICTIONARY_FILE=/var/lib/scrabble/words.txt
SCRABBLE_DICTIONARY_NAME=deployment-list
SCRABBLE_DICTIONARY_REVISION=2026.1
```

The file must be a regular UTF-8 file no larger than 64 MiB, with one exact uppercase alphabetic word per line. Blank lines are ignored; spaces, comments, punctuation and lowercase words are rejected with a line number. Production requires an absolute path. Loading completes once before factory initialization/listener startup; immutable validated content is shared by every created/recovered match. No word-list path or content enters the static bundle or browser metadata. Operators supply a word list they are entitled to use; this repository ships only its small authored fixtures and never downloads or redistributes a third-party list.

The configured revision becomes `<operator revision>+sha256:<sorted word-set digest>`. The English board/scoring profile remains version one, with ruleset revision `1+dictionary-sha256:<dictionary identity digest>`. Changing name, revision or words therefore changes explicit match/replay identity. Equivalent order/duplicates/line endings preserve it. Existing factories retain their loaded list despite subsequent file changes; new factories load the changed content. Recovery fails closed under a changed identity. Restore the original list/configuration to recover those matches. The browser recognizes this version-one English profile, retains the selected identities and rejects changes during a connection; additional language/rules profiles follow in #30.

## Evidence

Native tests cover pluggable acceptance, structural-before-lookup ordering, identity mismatch, every invalid cross word, stable structured feedback, exact sets, invalid configuration, content fingerprints, immutable factory inputs, replay and recovery compatibility. The built Pages test uses an independently configured authored list that rejects AT and accepts TA/CTA, verifies private rejection without disconnect, then completes consecutive commits, pass, exchange and refresh through the real server. The development suite preserves the default authored-dictionary path.

Full gate: `bun run check`. Runtime source revision: `a83ebc131ff4b168cd2a1a7035b676a32a191412` ([game-server PR #34](https://github.com/moritzbrantner/game-server/pull/34)). Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
