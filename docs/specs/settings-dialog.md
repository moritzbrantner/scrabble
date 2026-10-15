# Settings dialog and visible preferences

Authority: Decision Atlas **2.2** (non-disruptive settings dialog), **2.3** (immediate interface language and appearance controls) and **2.5** (distinct shared-display and player layouts), tracked by issue #95. This contract was written before the implementation. It replaces the inline `<details>` Settings disclosure described as the baseline in `mobile-accessibility.md`.

## Page chrome

Every application route renders one compact preferences chrome: the shared board (fixture and connected), the creation route, the player join and joined-player routes, and unavailable/invalid error routes.

- **Gear button.** A button whose accessible name is the translated Settings title ("Settings", "Einstellungen", "Ajustes") shows a recognizable gear icon (an inline `svg` or `img`; a visible text label is optional). At the top of the page its box lies at the top right of the viewport: its top edge is within 64 CSS px of the viewport top and its right edge within 96 CSS px of the viewport right edge. It is at least 44×44 CSS px.
- **Directly visible selectors.** The interface-language selector ("Interface language": English, Deutsch, Español) and the appearance selector ("Appearance": Light, Dark, System) are visible native selects (`combobox` role) without opening anything. At the top of the page both are inside the viewport, at least 44 CSS px tall, and neither causes horizontal page scrolling at 320, 390 or 1280 CSS px wide.
- **No inline disclosure.** No `<details>`/`<summary>` or other inline expansion is used for Settings. No dialog is open initially.
- The chrome does not cover gameplay. A player can still tap rack tiles, board squares and the commit action, which must be the topmost element at their centre.

## Settings dialog

- Activating the gear (pointer, Enter or Space) opens a **modal** dialog: `role="dialog"` with `aria-modal="true"`, or a native `<dialog>` opened with `showModal()`. Its accessible name is the translated Settings title, which is also shown as its heading.
- Opening it neither navigates nor reflows the game. The URL is unchanged. The board or editor behind it keeps its exact position and remains rendered underneath.
- Focus moves into the dialog on open and stays there. Repeated Tab and Shift+Tab never move focus outside the dialog or to the document body.
- An explicit close button is named with the translated "Close" ("Close", "Schließen", "Cerrar"). The dialog closes with Escape or this button, and then focus returns to the gear. Escape in an open dialog is consumed by the dialog. It must not reach gameplay keyboard handling.
- Content (presentation settings only):
  - It always contains an explanatory note. In English the note contains the phrase "does not change the word list". The full sentence is "The interface language changes labels and messages only. It does not change the word list, tiles or match rules." Translated German and Spanish notes are required.
  - It may repeat the interface-language and appearance selectors. Repeated selectors are bound to the single preferences state, so their value equals the chrome value.
  - It may hold other **presentation** options. It must not contain gameplay, rules, dictionary, tile, board or clock settings. The only `combobox` controls it may contain are the two preference selectors, and it contains no `textbox` or `spinbutton`.
  - On the shared board it never shows a private rack or player-only editing controls (Decision 2.5).
- With reduced motion requested, the opened dialog has no running animations.
- Light and dark appearance visibly change the dialog: its computed background/foreground pair differs between Light and Dark.
- A storage-failure message caused while the dialog is open must be perceivable while it stays open. The alert must not be hidden behind the modal.

## Preserved state and semantics

- One preferences source: `{locale, theme}` in `localStorage["scrabble:preferences:v1"]`, plus the single `lang` URL parameter, keep their existing meaning (`mobile-accessibility.md`). Language changes rewrite only `lang` in the current URL. Other query parameters (`server`, `api`, `certificate`, `match`, `view`) are unchanged.
- Opening or closing the dialog, and changing either visible preference, on the shared board or on a joined player controller of a real native match does not:
  - create a new WebTransport connection (connection count stays 1);
  - clear the tentative draft on the controller or the preview on the shared board;
  - change the player's session storage (identity and credential) or connection status. The transport `sequence` counter in the session entry is not identity: it advances while the draft preview is re-published, so it may only grow;
  - change the private rack, the authoritative phase/active turn, the ruleset or the dictionary.
- Interface language changes labels and messages only. It never changes the word list, tile distribution or rules.
- Blocked storage keeps the page usable and announces "Preferences could not be saved. They apply to this page." with `role="alert"`. Unsupported `lang` values and corrupt stored preferences fall back to English, and the gear is then named "Settings".
- Theme selection persists, and System follows the operating-system scheme.

## Accessibility

Axe reports no violations, without rule exclusions, with the dialog closed and open. This holds on the shared board, the creation and error routes and a joined native player, at desktop (1280×900) and mobile (390×844) viewports.

## Verification

`apps/web/browser/settings-dialog.spec.ts` runs in the development browser suite (`bun run test:browser`) against the real native transport/game fixture. Its tests:

1. **Gear chrome on every route.** Desktop and mobile check gear placement and size, visible selectors, the absence of `<details>`, open/close and axe for the shared board, creation and error routes, plus 320 px for the shared board. Screenshots are attached for each route and viewport, with the dialog closed and open.
2. **Modal semantics and keyboard.** Checks modal role and name, focus entry, the Tab/Shift+Tab trap, Escape and the close button, focus return, a stable board position and URL, the note, the allowed content only and reduced motion.
3. **Translated gear and dialog.** Checks German and Spanish names (gear, dialog, close), an English fallback for `lang=fr`, and a live language switch from the chrome while on the page.
4. **Appearance changes the dialog.** The Light and Dark dialog colours differ. Light and dark screenshots are attached.
5. **Blocked storage while open.** The storage alert is announced. If the dialog repeats the appearance selector, changing it there leaves an alert that is the topmost element at its centre.
6. **Native match invariants.** Uses a real board and two phones (390×844, reduced motion) with a two-tile draft. The phone and shared board each open and close via the keyboard and the close button. They switch language and appearance from the visible chrome. Connection count, draft/preview, session identity/credential (the `sequence` counter may only grow), rack, phase, ruleset, dictionary and non-`lang` URL parameters all remain unchanged. The test checks that gameplay targets are not obstructed and runs axe with the dialog open and closed. Mobile and desktop screenshots are attached.

Screenshots are test attachments (`testInfo.attach`), not committed baselines.

### Existing tests changed by this contract

Decision 2.2/2.3 replaces the old "click the Settings text to expand an inline section, then choose a language" step. These edits make existing tests use the directly visible language selector. They also query both preference selectors by `combobox` role, so a dialog that repeats them (hidden while closed) does not create ambiguous matches:

- `preferences.spec.ts`
  - Removes the `getByText("Einstellungen"/"Ajustes"/"Settings").click()` expansion steps.
  - Replaces the `.app-preferences details` "not open" assertion with "no dialog is open".
  - Queries language and appearance by `combobox` role.
- `reconnect.spec.ts` and `profile-creation.spec.ts`
  - Remove the same `getByText("Settings").click()` expansion step.
  - Query the language selector by `combobox` role.

The behaviour they prove (persistence, fallback, storage failure, no reconnect, unchanged profile choices) is unchanged.

## Open questions

- None that block this issue. Atlas 2.2 lets additional presentation options live in the dialog. This contract only requires the explanatory note and allows the two preference selectors to be repeated. Whether the owner wants further presentation options there (e.g. motion or board-label preferences) is a separate product choice and out of scope for #95.
