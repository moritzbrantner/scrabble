# Dictionary hints and estimated scores

Owner decisions: Atlas 1.53 (hints are an optional match setting, off by default), 1.54 (waiting pre-moves are estimated locally, with no hints), 1.55 (square and hex boards both show an estimate). Issue #165.

## Match setting

- Match setup has a **Dictionary hints** checkbox. It is unchecked for every new match. Turning it on sends `rules.dictionary_hints: true` in the v3 creation request.
- The value becomes part of the immutable composed `Ruleset` (`dictionary_hints`). It feeds the configuration identity, the idempotent-retry conflict check, and recovery/replay. When off it is omitted, so existing configurations, identities and snapshots stay byte-identical.

## Private estimates

- **Active turn:** the existing private preview (`own_preview`, #155) now carries a server evaluation for every board shape. It runs the same structural validation and scoring as a commit. With hints **off** it skips only the dictionary membership check, so the result is an estimate that reveals nothing about word validity. With hints **on** it also checks membership against the match's actual dictionary, and invalid words are reported privately.
- **Waiting pre-move:** the browser computes a non-authoritative estimate locally (`score-estimate.ts`), mirroring native word discovery, premium policy, main-word metric and bingo. It has no dictionary and therefore no hints, even in a hints-enabled match.
- The UI always labels the number **Estimated score**. With hints off it says the dictionary is checked at commit. With hints on and every word valid it says so. On square boards, structural problems keep using the existing local guidance.
- A commit is always checked against the dictionary and scored by the server, whatever the setting. Neither an estimate nor a hint ever reaches another player, the shared board or the public wire.

## Evidence

- Native: `dictionary_hints_gate_membership_in_private_estimates_but_never_commit_checks`. With hints off, an invalid word's preview is a `valid` estimate with the score breakdown. With hints on, it is `invalid_words`. In both cases the commit is rejected with `InvalidWords`. The setting is omitted from the default ruleset.
- Browser parity: the Rust draft oracle now emits the native score of each valid case. `move-draft-parity.test.ts` checks that the local estimate matches it exactly (square boards).
- Browser: `dictionary-hints.spec.ts` runs two simultaneous real matches, one with the default setting and one with hints enabled. Only the enabled match gives membership feedback; the default one shows the estimate and the "checked when you commit" note. Neither leaks to the other phone or the board. `hexagonal-board.spec.ts` now expects an estimate, not an invalid-word verdict, before an invalid crossing is committed in a hints-off match.
- **Unverified:** the hex-board local (waiting) estimate has no native oracle, and match restart/recovery with hints enabled has no dedicated test, though the setting is persisted in the composed ruleset.
