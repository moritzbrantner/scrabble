# Scrabble Decision Atlas

This is the canonical owner-controlled source for **what the Scrabble product should do**. Existing `docs/specs/` describe detailed acceptance, `docs/adr/` record exceptional architecture trade-offs, and GitHub issues track implementation. A recorded Decision is not a claim that the feature is implemented or independently verified.

## Editing rules

- **Decision** and **Question** are the only semantic leaf kinds; headings organize the product by feature.
- Numbered Decision IDs remain stable. An entry without `[question]` is a Decision approved by the owner. An entry marked `[question]` is an unresolved product choice; it does not block unrelated work.
- When a Question is answered, keep its ID and append a Decision citing `Answers <ID>`. When intent changes, append a Decision citing `Supersedes <ID>`; do not silently overwrite historical intent.
- Do not derive new owner approvals from existing code, specifications, merged PRs, or tests. Implementation agents should ask only about consequential unanswered Questions affecting their task.
- Keep product direction here and implementation detail in code/tests. Reference stable IDs from implementation issues instead of duplicating the same product decision.

Source: owner free-form steering session, 2026-10-13. Previous completed implementation and acceptance specifications are evidence of the current baseline, not proof of additional owner approval.

## 1. Match rules and variants

- **1.1 Configurable word lists and game languages.**  
  Status: owner-approved 2026-10-13.  
  The game should let people define and choose between different word lists for their matches, including the possibility of lists for different game languages. Match language and dictionary are game settings, **not** the interface language preference in 2.3. Existing deployment-configured dictionaries are a foundation, not a substitute for the intended player-facing configuration.

- **1.2 Configurable tile sets.**  
  Status: owner-approved 2026-10-13.  
  Different game variants may define which letters appear on tiles and their frequency and values. A match uses one selected, fixed set of rules; a configuration change does not retroactively alter ongoing matches or replays. The precise representation of any multi-letter tiles remains open in 1.7.

- **1.3 Keep conventional premium types for now.**  
  Status: owner-approved 2026-10-13.  
  Double-letter, triple-letter, double-word and triple-word premiums remain the supported premium categories while trying other variants. Do not invent extra premium categories as a prerequisite for new boards or word lists.

- **1.4 Experiment with different board layouts, including hexagons.**  
  Status: owner-approved 2026-10-13.  
  The game should make it possible to try different board configurations beyond the existing square layout. A hexagonal board is a desired experimental **game mode**, not merely a different graphic for unchanged square-grid rules. Hexagonal word-placement and scoring choices were subsequently answered by Decisions 1.10–1.13 (Question 1.6).

- **1.5** [question] **How do players define and make word lists available?**  
  Answered 2026-10-13 by 1.8 for list sources and reuse; list ownership/sharing details remain in 1.9.  
  Should word lists be selected from server-provided options, pasted/uploaded by the host, managed as reusable custom profiles, or some combination? Who may provide a list, and what validation, provenance and sharing rules apply? Current implementation supplies profiles at deployment time; this Question must not be silently answered by that implementation.

- **1.6** [question] **What constitutes a legal word on a hexagonal board?**  
  Resolved 2026-10-13 by Decisions 1.10 (three straight word axes), 1.11 (validate and score every newly formed crossing word), 1.12 (opening word covers the center hexagon), and 1.13 (board-variant premium policy). Retained as a historical Question. Hexagonal play is not merely square-grid logic drawn with different geometry.

- **1.7** [question] **Are multi-letter tiles part of the configurable tile-set goal?**  
  Does “different tiles with different letters” mean one configurable character per tile (including Unicode letters such as Ä), or should a tile optionally represent several characters such as CH? The existing native ruleset supports individual Unicode scalar tile faces.

- **1.8 Predefined and reusable custom word lists.**  
  Status: owner-approved 2026-10-13. Answers 1.5.  
  Players can choose from predefined word lists, upload their own, and save uploaded lists for use in later matches. The selected list defines valid words for its match independently of the interface language. Do not treat existing server-only dictionary configuration as satisfying the requested player-facing import and reuse. This Decision does not settle how a player's saved lists are scoped or shared (see 1.9).

- **1.9** [question] **Who can access and reuse saved custom word lists?**  
  Should a saved upload be reusable only in the same browser/device, associated with a player's identity across devices, or shareable with other hosts? Define who can view, change or delete saved lists without compromising dictionaries required to replay or recover ongoing matches. This does not block selecting existing profiles under 1.1.

- **1.10 Hexagonal words use three straight axes.**  
  Status: owner-approved 2026-10-13. Partially answers 1.6 (direction choice A).  
  Hexagonal Scrabble allows words to extend in a straight line along **any of the hexagon grid's three axes**. A single word does not bend or change axes. This retains Scrabble-like straight-word placement but permits additional crossing possibilities compared with a square board. Later Decisions 1.11–1.13 address crossing words, first placement, and premium policy (historical Question 1.6).

- **1.11 Every newly formed hexagonal crossing word counts.**  
  Status: owner-approved 2026-10-13. Partially answers 1.6 (crossing-rule choice A).  
  When a placement on a hexagonal board forms additional words along either of the other straight axes, **every newly formed word must pass the configured word-list validation, and every newly formed word scores points**, just as the main word does. A placement containing any invalid newly formed word cannot be committed. Decisions 1.12 and 1.13 subsequently settle first-turn placement and premium-policy selection (historical Question 1.6).

- **1.12 First hexagonal word covers the center.**  
  Status: owner-approved 2026-10-13. Partially answers 1.6 (opening-placement choice A).  
  On a hexagonal board, the opening word must include the board's center hexagon, following the familiar center-opening rule of conventional Scrabble. This does not permit opening elsewhere merely because the geometry is hexagonal. Any supported hexagonal board configuration needs a well-defined center for this rule. Decision 1.13 subsequently settles premium-policy selection (historical Question 1.6).

- **1.13 Board variants choose premium application.**  
  Status: owner-approved 2026-10-13. Answers the premium-intersection part of Question 1.6 (choice C), completing its recorded hexagonal rule choices together with 1.10–1.12.  
  Each hexagonal board variant specifies how its existing double/triple letter and word premiums apply when a newly placed tile forms multiple words: either **classic all-word application** (the premium affects every newly formed word containing that tile) or **main-word-only application** (the premium affects only the main word; crossing words still score without that premium). Both modes retain normal validation and scoring of every newly formed word under Decision 1.11. A premium is consumed by its newly placed tile after that move, not re-applied on later turns. The choice belongs to the selected board/ruleset configuration and remains fixed for an ongoing match; it is not a mid-match toggle. Existing classic square-board scoring is unchanged.

## 2. Game clients and interface controls

- **2.1 Mobile-style client experience across devices.**  
  Status: owner-approved 2026-10-13.  
  Gameplay clients should have an interaction layout resembling the mobile client even when used in a desktop web browser. Keep the play area and frequent actions primary rather than replacing them with a conventional desktop application shell. Whether the separate shared-board presentation is also covered remains unanswered in 2.4.

- **2.2 Non-disruptive settings dialog.**  
  Status: owner-approved 2026-10-13.  
  A recognizable gear icon at the top right opens a **modal** settings dialog, rather than navigating the user away from the match or expanding an inline settings section. Opening/closing the dialog must not reset game state, tentative placements, match connection or player identity. Additional presentation options may live within it.

- **2.3 Immediate interface language and appearance controls.**  
  Status: owner-approved 2026-10-13.  
  A language selector for **interface text** and a light/dark appearance control should remain directly discoverable in the game interface, without opening the settings modal. The existing system-theme option may remain. Switching interface language must not change the word list, language, tile distribution or other match rules.

- **2.4** [question] **Does the mobile-style layout also replace the dedicated shared-board view?**  
  The existing product has an independently presented shared public board and player-specific private-rack controls. Should the common board remain a distinct large-screen presentation, or should it adopt the same mobile-like client layout? Do not silently remove either role.

## 3. Match clocks

- **3.1 Optional configurable time controls.**  
  Status: owner-approved 2026-10-13.  
  Match setup should allow trying different time settings, including a total time allowance and an optional allowance added on starting a turn (30 seconds was an example, not an approved default). Clock configuration belongs to the match, not display-only browser preferences. Exact clock ownership and expiration semantics remain unresolved in 3.2–3.3.

- **3.2** [question] **What happens when a player's allotted time runs out?**  
  Choose the match-visible consequence (for example automatic pass, loss/forfeit, or another rule), including whether an expired clock ends the match immediately. No rule is approved yet.

- **3.3** [question] **What time is counted, and when is an allowance granted?**  
  Is total time per player, per turn, or shared? Is additional time granted when each turn starts, after completing it, or by another rule? How do pausing, disconnects, reconnects and server recovery affect elapsed time? These choices must be settled before a playable clock implementation.

## Existing baseline and boundaries

The current application has an authoritative Rust ruleset with deployment-selected dictionary/tile/board configuration, a separate shared-board and phone-player role, persisted English/German/Spanish interface translations, and a directly visible Light/Dark/System appearance selector. See `docs/specs/configurable-rulesets.md`, `docs/specs/mobile-accessibility.md`, `docs/adr/0007-shared-board-authority.md`, and [merged PR #92](https://github.com/moritzbrantner/scrabble/pull/92). These may fulfill parts of the above intent; they do **not** establish that player-selectable profiles, a gear modal, hexagonal play, or clocks are available today.
