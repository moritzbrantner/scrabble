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
  Different game variants may define which letters appear on tiles and their frequency and values. A match uses one selected, fixed set of rules; a configuration change does not retroactively alter ongoing matches or replays. Later Decision 1.21 authorizes multi-letter tile faces (historical Question 1.7).

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
  Answered 2026-10-13 by Decision 1.21 (choice B); retained as a historical Question.  
  Does “different tiles with different letters” mean one configurable character per tile (including Unicode letters such as Ä), or should a tile optionally represent several characters such as CH? The existing native ruleset supports individual Unicode scalar tile faces.

- **1.8 Predefined and reusable custom word lists.**  
  Status: owner-approved 2026-10-13. Answers 1.5.  
  Players can choose from predefined word lists, upload their own, and save uploaded lists for use in later matches. The selected list defines valid words for its match independently of the interface language. Do not treat existing server-only dictionary configuration as satisfying the requested player-facing import and reuse. This Decision does not settle how a player's saved lists are scoped or shared (see 1.9).

- **1.9** [question] **Who can access and reuse saved custom word lists?**  
  Answered 2026-10-13 by Decision 1.16 (choice C: saved reusable lists are accessible through shareable links). Recipient permissions and original-owner management were subsequently answered in Decisions 1.18 and 1.20 (historical Questions 1.17 and 1.19). Retained as a historical Question.  
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

- **1.14** [question] **Which newly formed word is the main word after a single-tile placement?**  
  Answered 2026-10-13 by Decision 1.15 (option B). Retained as a historical Question.  
  When one tile completes multiple straight words along different hexagonal axes, Decision 1.13's main-word-only premium policy makes the main-word identity affect scoring. Should the player explicitly select the main direction, should the game pick the longest formed word with a deterministic tie-break, or should a fixed axis precedence decide automatically? The chosen main word and scores must be understandable before committing; do not silently assign a scoring-affecting direction.

- **1.15 Longest formed word is the single-tile main word.**  
  Status: owner-approved 2026-10-13. Answers 1.14 (choice B).  
  When placing one tile on a hexagonal board forms multiple words along different axes, the game **automatically selects the longest newly formed word as the main word**. If several words are equally long, use a deterministic tie-breaker that yields the same result across clients, replay, and recovery. Show the selected main word and its scoring consequences clearly before confirmation, particularly under Decision 1.13's main-word-only premium policy. Do not require the player to choose the direction. Decision 1.21 later approves multi-letter tile faces; Decision 1.23 subsequently settles the scoring-relevant length metric (historical Question 1.22).

- **1.16 Shareable saved word-list library.**  
  Status: owner-approved 2026-10-13. Answers 1.9 (choice C).  
  Custom word lists may be uploaded and saved for reuse in later matches, and players can **share a saved list with other players through a link**. A recipient can access the linked list for use in Scrabble rather than having to receive and re-upload its source file. The library should support reuse beyond one match or one browser session; the user-facing intention is a reusable shareable library, not deployment-only dictionaries. Sharing a list does not change an already running match's immutable selected dictionary or invalidate replay/recovery. The later Decisions 1.18 and 1.20 settle recipient copying and original-owner management (historical Questions 1.17 and 1.19).

- **1.17** [question] **What can someone with a shared word-list link do?**  
  Answered 2026-10-13 by Decision 1.18 for recipient behavior (choice C); original-list management was subsequently answered in Decision 1.20 (historical Question 1.19). Retained as a historical Question.  
  Should a shared link only grant access to select/use a saved list, grant collaborators editing permission, or give recipients independent copies they can modify? Also settle who can change/delete the original and how ownership is established without breaking dictionaries referenced by running or recoverable matches. Do not infer these permissions from Decision 1.16 alone.

- **1.18 Shared word lists can be copied independently.**  
  Status: owner-approved 2026-10-13. Answers 1.17 (recipient-permissions choice C).  
  A player opening a shared custom word-list link can **save an independent copy** and edit that copy for their own future games. Editing the recipient's copy must not modify the original shared list, other recipients' copies, or word-list versions selected by existing matches. A link can still provide access to use the shared original in a match under Decision 1.16; this choice does not grant collaborators write access to the original. Decision 1.20 subsequently settled guest ownership with optional account linking (historical Question 1.19).

- **1.19** [question] **How is ownership of a saved custom word list established and recovered?**  
  Answered 2026-10-13 by Decision 1.20 (choice C: guest ownership with optional account linking). Retained as a historical Question.  
  Should owners manage lists through an account across devices, an anonymous device-held capability, or guest ownership with an optional account upgrade? Define who can edit/delete or stop sharing the original and what happens when a device or credential is lost. Recipients' editable copies remain independent under 1.18. Existing matches must retain their selected immutable dictionary identity regardless of library changes. This choice must be settled before promising durable library management and owner controls.

- **1.20 Guest word-list ownership with optional account linking.**  
  Status: owner-approved 2026-10-13. Answers 1.19 (choice C).  
  Players can upload, save, use, edit, and manage their own reusable custom word lists **without creating an account first**. Guest ownership is retained by a private device-held ownership capability, not by the public sharing link. Players may optionally link their existing guest-owned lists to an account later, preserving those lists, independent copies, and their current share links rather than restarting their library; linked owners can manage their lists from their account on another device. Only the current owner may modify, delete, or stop sharing the original; a person using a share link may use it or save an independently editable copy under 1.18, not edit the owner's version. Losing an unlinked guest ownership capability may lose management access; the interface must communicate this without falsely promising recovery. Edits and deletes must not rewrite or invalidate immutable dictionary versions already bound to running or recoverable matches. The identity provider, account sign-in UX, and any cross-product account integration are not selected by this Decision.

- **1.21 Configurable multi-letter tile faces.**  
  Status: owner-approved 2026-10-13. Answers 1.7 (choice B).  
  A ruleset may define a tile whose face contains several letters, such as **CH** or **SCH**, alongside ordinary single-letter tiles. Each such tile is **one physical tile**: it takes **one rack slot**, occupies **one board cell**, and has **one configured point value**, irrespective of the number of letters printed on its face. The whole face contributes its letters in order to words formed along the board, and dictionary validation uses the resulting spelling. Tile-frequency and value choices remain part of the selected immutable match ruleset. Preserve the existing single-letter profiles and game authority. Decision 1.23 subsequently selects a configurable length metric with tile count as the default (historical Question 1.22).

- **1.22** [question] **What makes a word the longest when tiles can have multiple letters?**  
  Answered 2026-10-13 by Decision 1.23 (choice C with B as default). Retained as a historical Question.  
  Under Decision 1.15, a single tile forming several hexagonal words automatically chooses the longest as the main word. If one candidate uses CH or SCH tiles, should “longest” mean the number of spelled letters, the number of board cells/tiles, or a configurable ruleset setting? The answer changes scoring under the main-word-only premium policy in 1.13. Do not silently choose one until the owner answers.

- **1.23 Configurable main-word length with a tile-count default.**  
  Status: owner-approved 2026-10-13. Answers 1.22 (choice C, with B as the default).  
  Each ruleset may choose whether the **longest** word used for automatic hexagonal main-word selection under Decision 1.15 is determined by the **number of occupied board cells / physical tiles** or by the **number of letters in the fully spelled word**. **Default: count tiles**, so a CH or SCH tile contributes one unit to word length; a ruleset may explicitly opt into letter-counting, where CH contributes two letters and SCH three. The length metric is part of the selected immutable match ruleset and must produce the same main word, stable tie-break, scoring preview, committed result, replay and recovery. Every formed word is still validated against its full spelling under Decisions 1.11 and 1.21; this choice changes only main-word length comparison, not dictionary spelling, tile point value or board occupancy. Preserve existing square-board play and ruleset compatibility.

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
