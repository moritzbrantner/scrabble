# Phone rack and public scores

After admission and Start, the phone renders only the `own_rack` selected by its authenticated, validated player snapshot. Each tile keeps its authoritative ID, letter and ruleset-derived point value. A blank has a distinct diamond mark, an accessible “Blank tile, 0 points” label and value zero. Unknown letter values display an explicit unknown marker rather than inventing a score.

The rack is a semantic list. It is marked unavailable unless the phone is joined, connected, and the authoritative active player. The [move editor](move-editor.md) adds selection controls and tentative editing without text inputs. Waiting or disconnected clients cannot initiate placement from this view. No rack reordering, removal, replacement draw or turn advancement occurs optimistically. Every validated snapshot replaces the view's authoritative rack and public scores.

Public score rows preserve authoritative seat order, names and scores, and identify the active player visually. Phone layouts fit 320- and 390-pixel portrait screens and landscape. Narrow racks wrap into two rows to preserve at least 44 pixels of width per tile. Normal rack viewing never focuses an input or opens a keyboard.

## Evidence

`private_rack_fixtures` produces test-only player projections using Rust's actual deal, commit and refill operations, checks tile conservation, and is compared against the committed JSON fixture by the native gate. View tests cover letter values, a blank, removed committed tiles, newly drawn tiles, immutable source snapshots and exclusion of the other player's tile IDs. The authoritative commit transaction now refills the committing rack from the same seeded bag before publishing the next turn. Browser acceptance verifies two new identities, removal of the placed identities, an unchanged other rack, public bag count, and the exact replacement rack after refresh. Native tests cover full, partial and exhausted bags, tile conservation, replay and recovery.

Chromium starts the actual board and four phones, compares each DOM rack's IDs to its decoded private wire projection, verifies disjoint racks, public scores, active/waiting availability, disabled waiting rack controls and absence of text inputs. It checks portrait/landscape geometry and restores the exact rack after reload. Screenshots remain ignored local test artifacts. Full gate: `bun run check`.

Shared convention sourceRevision: `46d8793bb3034326561f876dcc67dbaa5aa1e432`.
