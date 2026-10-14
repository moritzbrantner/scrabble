# Authoritative match-profile composition

This headless prerequisite of #94 supports Atlas Decisions 1.25, 1.31 and 1.39.
It supplies one server-owned composition seam, using the existing canonical
`Ruleset` and immutable dictionary inputs. It does not make match-time profile
selection, hexagonal play or clocks available in the browser.

## Source profiles and effective configuration

Word-list, tile and board profiles have separate content identities. Validated
source profiles retain immutable content. Word sets normalize order, duplicate
entries and line endings through the existing `WordList` representation. Their
content fingerprints use the same server-owned normalized-word hashing boundary
as deployment dictionaries. Tile definitions retain their semantic order;
board premiums retain row-major order. Changing source content under unchanged
nominal name/revision must still change the effective content identity.

The currently supported board profile is an odd square of at most 15×15 with
one premium per cell and a unique center. This does not invent hexagonal rules
or claim those rules are supported. Future #96 geometry must extend the same
authoritative board boundary rather than emulate hexagons in React.

The composition caller explicitly supplies the existing rack size, bingo bonus,
exchange minimum, scoreless-turn limit and player limits. The seam introduces
no new defaults or wider numeric bounds. It validates the entire combination:
letters in the selected word list must be supported by the selected tile
alphabet; existing board/tile/rack/player limits and tile conservation
requirements continue to apply. Incompatible combinations return a structured
reason without substituting a source or changing another composition.

The result retains all three selected source identities, the immutable shared
word list and an ordinary effective `Ruleset`. Its deterministic composite
identity covers all source identities/content and every effective rule. The
existing `GameState`/`GameSession` remain the only rules authority and can consume
the result directly. Canonical state retains the complete effective ruleset;
existing game serialization and replay contracts remain unchanged.

## Verification and remaining product acceptance

Separate-context native acceptance tests exercise the public composition seam,
real game-state construction and canonical serialization/reconstruction. They
must cover independent compatible combinations, invalid inputs, Unicode
alphabet compatibility, content-bound identities, effective rule changes and
unchanged earlier configurations. Existing deployment dictionary fingerprints
and square-game acceptance must remain compatible.

The actual catalog, three browser selectors, advanced match-settings form,
versioned HTTP creation, per-match server recovery and concurrent hosting still
belong to #94. The complete public hexagonal-board journey remains #96, including
three-axis placement, both premium policies, scoring previews and true hex hit
targets. Neither issue can close from this native prerequisite's tests. Physical
phone acceptance and controlled performance acceptance remain separate.
