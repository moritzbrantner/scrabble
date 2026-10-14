# Independent match-profile composition

Accepted technical boundary for Atlas Decisions 1.25, 1.31 and 1.39; prerequisite
#107 of #94/#96.

The server owns immutable source-profile validation, content fingerprints and
composition. The game crate remains the canonical owner of supported tile,
board and effective rules invariants. Separate word-list, tile and board source
identities are bound to one effective Ruleset; changing any selected identity,
source content or effective rule changes the composite fingerprint. Existing
deployment dictionary fingerprints keep their compatibility contract.

Source constructors prepare dictionaries and alphabets once. Composition shares
the immutable word list rather than parsing or copying it for every match, and
returns owned effective configuration to the existing game engine. It cannot
allocate a hosted match or substitute incompatible selections.

This boundary currently supports the existing square geometry and single-letter
tile faces. It supplies no competing browser game rules. Catalog discovery,
creation requests, match-local server recovery and frontend selectors remain
#94 responsibilities; actual hex geometry/scoring remains #96. No public
product acceptance is implied by this native technical enabler.
