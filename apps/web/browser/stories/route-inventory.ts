// The real routing surface, read from apps/web/src (App.tsx is the only router). The app has one
// path, the Vite base `/scrabble/`; every view is chosen by query parameters. `scripts/
// story-coverage.test.ts` fails when a parameter read in the source is missing here.

export type InventoryRoute = {
  url: string;
  view: string;
  /** ROUTE-NAV model state, or why it has none. */
  state?: string;
  notCovered?: string;
};

export const routeInventory: readonly InventoryRoute[] = [
  { url: "?server&api[&certificate]", view: "Create game", state: "create" },
  { url: "?server&api&view=<unknown>", view: "Create game (fallback)", state: "unknownView" },
  { url: "?server&api&match=<created here>", view: "Live board", state: "board" },
  { url: "?server&match=<unknown>", view: "Board recovery alert", state: "boardUnknownMatch" },
  { url: "?server&match=<malformed>", view: "Connection alert", state: "boardMalformedMatch" },
  { url: "?match=<id> (no server, dev)", view: "Connection alert", state: "boardWithoutServer" },
  { url: "?server=<bad>&api=<bad>", view: "Connection alert", state: "connectionInvalid" },
  { url: "?view=player&match=<open>", view: "Join form", state: "join" },
  { url: "(same URL, seated in this tab)", view: "Player phone", state: "joined" },
  { url: "?view=player", view: "Invalid invite alert", state: "inviteNoMatch" },
  { url: "?view=player&match=<malformed>", view: "Invalid invite alert", state: "inviteMalformed" },
  { url: "?view=player&match&match", view: "Invalid invite alert", state: "inviteAmbiguous" },
  { url: "?view=player&match=<expired>", view: "Expired alert", state: "inviteExpired" },
  { url: "?view=player&match=<unknown>", view: "Unavailable alert", state: "inviteUnknown" },
  {
    url: "?view=player&match=<started>",
    view: "Already started alert",
    notCovered: "reached in LOBBY-1 (LATE_JOIN_REFUSED), not as a direct-load route",
  },
  { url: "?view=word-list&share=<shared>", view: "Shared word list", state: "wordListShared" },
  { url: "(after Use for a new game)", view: "Create with list", state: "wordListInUse" },
  { url: "?view=word-list&share=<missing>", view: "Unavailable alert", state: "wordListMissing" },
  { url: "?view=word-list&share=../x", view: "Unavailable alert", state: "wordListMalformed" },
  {
    url: "?view=layout&share=<shared>",
    view: "Shared board layout",
    notCovered: "needs an authored layout document; custom-board-layouts.spec.ts covers it",
  },
  { url: "?view=layout&share=<missing>", view: "Unavailable alert", state: "layoutMissing" },
  { url: "?launch=<other>&session", view: "Launch alert", state: "launchInvalid" },
  { url: "?launch=moenarch&session", view: "Launch alert", state: "launchUnbound" },
  { url: "?launch=moenarch&session&view=board", view: "Create game", state: "launchHosting" },
  {
    url: "?launch=moenarch&session&view=board&match",
    view: "Linked board",
    notCovered: "needs a launcher opener window; moenarch-launch.spec.ts covers it",
  },
  { url: "(no query, dev only)", view: "Lobby board preview", state: "preview" },
  { url: "?fixture=playing|finished, dev only", view: "Board preview", state: "previewPlaying" },
  { url: "?fixture=<unknown>, dev only", view: "Lobby preview", state: "previewBogusFixture" },
  {
    url: "(no service configured, production)",
    view: "Service unavailable alert",
    notCovered: "production build only; production-configuration.spec.ts covers it",
  },
  {
    url: "/scrabble/<unknown path>",
    view: "Host 404",
    notCovered:
      "decided by the static host, not the app. Checked by hand 2026-10-31: 404 on GitHub Pages and moenarch.com. The dev server serves the app instead.",
  },
];

export const queryParameters: Readonly<Record<string, string>> = {
  view: "ROUTE-NAV: player, word-list, layout, board and unknown values",
  match: "ROUTE-NAV, ROUTE-INVITE: valid, missing, malformed, duplicated, expired, unknown",
  share: "ROUTE-NAV: valid word list, missing, malformed; valid layout not reached",
  lang: "ROUTE-QUERY: applied, reloaded, UI update, unsupported, duplicated; ROUTE-INVITE: forwarded",
  fixture: "ROUTE-NAV: playing, unknown; finished not reached (shared-board.spec.ts covers it)",
  launch: "ROUTE-NAV: valid and invalid",
  session: "ROUTE-NAV: valid; a malformed value is not reached",
  server: "ROUTE-NAV: valid and invalid; ROUTE-INVITE: encoded (dev and test builds only)",
  api: "ROUTE-NAV: valid and invalid; ROUTE-INVITE: encoded (dev and test builds only)",
  certificate: "ROUTE-INVITE: forwarded; an invalid value is not reached (dev only)",
};
