// One small model per user-story area. Each model states documented intent, cited here, and
// is deliberately independent of application code (Decision Atlas 6.3).
import { assign, setup, type AnyStateMachine } from "xstate";

export type Story = {
  id: string;
  title: string;
  sources: readonly string[];
  model: AnyStateMachine;
  events: readonly string[];
};

// ---------------------------------------------------------------------------------- LOBBY-1

const lobbyEvents = [
  "CREATE",
  "BOARD_RELOAD",
  "ADA_JOINS",
  "ADA_RELOADS",
  "BAD_NAME_REJECTED",
  "LIN_JOINS",
  "SECOND_ADA_JOINS",
  "START",
  "LATE_JOIN_REFUSED",
] as const;
const lobby = setup({
  types: {} as { events: { type: (typeof lobbyEvents)[number] } },
}).createMachine({
  id: "lobby",
  initial: "noGame",
  states: {
    noGame: { on: { CREATE: "empty" } },
    empty: { on: { ADA_JOINS: "oneSeated", BOARD_RELOAD: "empty", BAD_NAME_REJECTED: "empty" } },
    oneSeated: {
      on: { LIN_JOINS: "twoSeated", SECOND_ADA_JOINS: "twoSeated", ADA_RELOADS: "oneSeated" },
    },
    twoSeated: { on: { START: "started" } },
    started: { on: { LATE_JOIN_REFUSED: "started" } },
  },
});

// ----------------------------------------------------------------------------------- TURN-1

const turnEvents = [
  "PLACE_AT",
  "PLACE_TA",
  "COMMIT_REJECTED",
  "CANCEL_DRAFT",
  "COMMIT",
  "OPEN_PASS",
  "KEEP_PLAYING",
  "CONFIRM_PASS",
  "OPEN_EXCHANGE",
  "KEEP_PLAYING_EXCHANGE",
  "CONFIRM_EXCHANGE",
] as const;
const turn = setup({
  types: {} as { events: { type: (typeof turnEvents)[number] } },
}).createMachine({
  id: "turn",
  initial: "adaTurn",
  states: {
    adaTurn: { on: { PLACE_AT: "draftAt", PLACE_TA: "draftTa", OPEN_EXCHANGE: "exchanging" } },
    draftTa: { on: { COMMIT_REJECTED: "rejected" } },
    rejected: { on: { CANCEL_DRAFT: "adaTurn" } },
    draftAt: { on: { COMMIT: "committed", OPEN_PASS: "passConfirm" } },
    passConfirm: { on: { KEEP_PLAYING: "draftAt", CONFIRM_PASS: "passed" } },
    exchanging: { on: { KEEP_PLAYING_EXCHANGE: "adaTurn", CONFIRM_EXCHANGE: "exchanged" } },
    committed: {},
    passed: {},
    exchanged: {},
  },
});

// --------------------------------------------------------------------------------- FINISH-1

const finishEvents = [
  "PASS_OUT",
  "PHONE_RELOADS",
  "BOARD_RELOADS",
  "START_REMATCH",
  "ADA_FOLLOWS",
  "LIN_FOLLOWS",
  "START_NEXT",
] as const;
const finish = setup({
  types: {} as { events: { type: (typeof finishEvents)[number] } },
}).createMachine({
  id: "finish",
  initial: "playing",
  states: {
    playing: { on: { PASS_OUT: "finished" } },
    finished: {
      on: {
        START_REMATCH: "rematchLobby",
        BOARD_RELOADS: "finishedElsewhere",
        PHONE_RELOADS: "finished",
      },
    },
    finishedElsewhere: {},
    rematchLobby: { on: { ADA_FOLLOWS: "adaFollowed" } },
    adaFollowed: { on: { LIN_FOLLOWS: "bothFollowed" } },
    bothFollowed: { on: { START_NEXT: "rematchPlaying" } },
    rematchPlaying: {},
  },
});

// -------------------------------------------------------------------------------- ROUTE-NAV

/** Views reachable by loading a URL directly. `create` is the start of every path. */
export const directRoutes = [
  "preview",
  "previewPlaying",
  "previewBogusFixture",
  "unknownView",
  "connectionInvalid",
  "boardWithoutServer",
  "boardUnknownMatch",
  "boardMalformedMatch",
  "join",
  "inviteNoMatch",
  "inviteMalformed",
  "inviteAmbiguous",
  "inviteExpired",
  "inviteUnknown",
  "layoutMissing",
  "wordListMissing",
  "wordListMalformed",
  "wordListShared",
  "launchInvalid",
  "launchUnbound",
  "launchHosting",
] as const;
type History = { stack: string[]; index: number; reloaded: boolean };
const routeEvents = [
  ...directRoutes.map((route) => `OPEN_${route}`),
  "CREATE",
  "FOLLOW_INVITE",
  "JOIN",
  "USE_WORD_LIST",
  "RELOAD",
  "BACK",
  "FORWARD",
];
const routeNames = [
  "create",
  "board",
  "joined",
  "wordListInUse",
  ...directRoutes,
] as readonly string[];
const canOpen = ({ context }: { context: History }) =>
  context.stack.length === 1 && context.stack[0] === "create" && !context.reloaded;
const moved = (stack: string[], index: number): History => ({ stack, index, reloaded: false });
/**
 * States are views and the context is the tab's session history, bounded to two entries, so
 * BACK and FORWARD are real graph edges. `reloaded` makes "navigate, reload, back" a different
 * vertex from "navigate, back": a generator that only visits states would otherwise skip it.
 */
const routes = setup({
  types: {} as { context: History; events: { type: string } },
}).createMachine({
  id: "routes",
  initial: "create",
  context: { stack: ["create"], index: 0, reloaded: false },
  on: {
    ...Object.fromEntries(
      directRoutes.map((route) => [
        `OPEN_${route}`,
        {
          guard: canOpen,
          target: `.${route}`,
          actions: assign(({ context }) => moved([...context.stack, route], 1)),
        },
      ]),
    ),
    // A reload does not move in history. Once per visit to the newest entry keeps this bounded.
    RELOAD: {
      guard: ({ context }) => !context.reloaded && context.index === context.stack.length - 1,
      actions: assign({ reloaded: true }),
    },
    BACK: routeNames.map((route) => ({
      guard: ({ context }: { context: History }) => context.stack[context.index - 1] === route,
      target: `.${route}`,
      actions: assign(({ context }: { context: History }) =>
        moved(context.stack, context.index - 1),
      ),
    })),
    FORWARD: routeNames.map((route) => ({
      guard: ({ context }: { context: History }) => context.stack[context.index + 1] === route,
      target: `.${route}`,
      actions: assign(({ context }: { context: History }) =>
        moved(context.stack, context.index + 1),
      ),
    })),
  },
  states: {
    create: {
      on: {
        // Creation replaces the entry (board-lobby.md), so it adds nothing to back.
        CREATE: {
          guard: canOpen,
          target: "board",
          actions: assign(() => moved(["board"], 0)),
        },
      },
    },
    board: {
      on: {
        FOLLOW_INVITE: {
          guard: ({ context }) => context.stack.length === 1,
          target: "join",
          actions: assign(({ context }) => moved([...context.stack, "join"], 1)),
        },
      },
    },
    join: {
      on: {
        // Seating happens on the same URL, so it replaces the join entry's meaning.
        JOIN: {
          guard: ({ context }) => context.stack[0] === "board" && context.index === 1,
          target: "joined",
          actions: assign(({ context }) => moved([context.stack[0] ?? "", "joined"], 1)),
        },
      },
    },
    joined: {},
    wordListShared: {
      on: {
        USE_WORD_LIST: {
          guard: ({ context }) => context.index === 1,
          target: "wordListInUse",
          // The entry's URL becomes the plain create page, which FORWARD would then load.
          // Marked reloaded to leave RELOAD out: whether this in-memory selection should survive
          // a reload is an open owner question.
          actions: assign(({ context }) => ({
            ...moved([context.stack[0] ?? "", "create"], 1),
            reloaded: true,
          })),
        },
      },
    },
    wordListInUse: {},
    ...Object.fromEntries(
      directRoutes
        .filter((route) => route !== "join" && route !== "wordListShared")
        .map((route) => [route, {}]),
    ),
  },
});

// ------------------------------------------------------------------------------ ROUTE-QUERY

const languageEvents = [
  "OPEN_PLAIN",
  "OPEN_LANG_DE",
  "OPEN_LANG_UNSUPPORTED",
  "OPEN_LANG_AMBIGUOUS",
  "SELECT_DE",
  "SELECT_EN",
  "RELOAD",
] as const;
/** An English choice and no choice behave alike here, so one flag keeps the graph small. */
type Saved = { saved: "none" | "de" };
const savedIsDe = ({ context }: { context: Saved }) => context.saved === "de";
const language = setup({
  types: {} as { context: Saved; events: { type: (typeof languageEvents)[number] } },
}).createMachine({
  id: "language",
  initial: "english",
  context: { saved: "none" },
  on: {
    // A single supported `lang` wins, an unsupported one is English, and otherwise the saved
    // preference applies (mobile-accessibility.md, settings-dialog.md).
    OPEN_LANG_DE: ".german",
    OPEN_LANG_UNSUPPORTED: ".english",
    OPEN_PLAIN: [{ guard: savedIsDe, target: ".german" }, { target: ".english" }],
    OPEN_LANG_AMBIGUOUS: [{ guard: savedIsDe, target: ".german" }, { target: ".english" }],
  },
  states: {
    english: {
      on: {
        SELECT_DE: { target: "german", actions: assign({ saved: "de" as const }) },
        RELOAD: "english",
      },
    },
    german: {
      on: {
        SELECT_EN: { target: "english", actions: assign({ saved: "none" as const }) },
        RELOAD: "german",
      },
    },
  },
});

// ----------------------------------------------------------------------------- ROUTE-INVITE

const inviteEvents = ["CREATE", "OPEN_LINK", "RELOAD", "BACK"] as const;
const invite = setup({
  types: {} as { events: { type: (typeof inviteEvents)[number] } },
}).createMachine({
  id: "invite",
  initial: "germanCreate",
  states: {
    germanCreate: { on: { CREATE: "germanBoard" } },
    germanBoard: { on: { OPEN_LINK: "germanJoin" } },
    germanJoin: { on: { RELOAD: "germanJoinReloaded" } },
    germanJoinReloaded: { on: { BACK: "germanBoardAgain" } },
    germanBoardAgain: {},
  },
});

export const stories: readonly Story[] = [
  {
    id: "LOBBY-1",
    title: "Create a game, invite players, join and start",
    sources: [
      "docs/specs/board-lobby.md",
      "docs/specs/player-invites.md",
      "docs/specs/phone-joining.md",
      "issue #34 minimal user flow",
      "Decision Atlas 2.5",
    ],
    model: lobby,
    events: lobbyEvents,
  },
  {
    id: "TURN-1",
    title: "Play a turn: commit, rejected word, pass or exchange",
    sources: [
      "docs/specs/playable-loop.md",
      "docs/specs/pass-turn.md",
      "docs/specs/tile-exchange.md",
      "Decision Atlas 2.6, 2.8",
    ],
    model: turn,
    events: turnEvents,
  },
  {
    id: "FINISH-1",
    title: "Finish by passes, see results, rematch and follow it",
    sources: [
      "docs/specs/game-completion.md",
      "docs/specs/rematch.md",
      "issue #34 minimal user flow",
    ],
    model: finish,
    events: finishEvents,
  },
  {
    id: "ROUTE-NAV",
    title: "Every view loads directly, survives reload, back and forward, and guards bad URLs",
    sources: [
      "owner request 2026-10-31 (routing as a first-class story)",
      "docs/specs/player-invites.md",
      "docs/specs/board-lobby.md",
      "docs/specs/shared-board.md",
      "docs/specs/moenarch-launch.md",
      "docs/specs/word-list-library.md",
    ],
    model: routes,
    events: routeEvents,
  },
  {
    id: "ROUTE-QUERY",
    title: "The lang parameter applies, persists, updates and falls back",
    sources: ["docs/specs/mobile-accessibility.md", "docs/specs/settings-dialog.md"],
    model: language,
    events: languageEvents,
  },
  {
    id: "ROUTE-INVITE",
    title: "Invite links encode their values and forward only allowlisted parameters",
    sources: ["docs/specs/player-invites.md", "docs/specs/game-creation.md"],
    model: invite,
    events: inviteEvents,
  },
];

export function story(id: string): Story {
  const found = stories.find((candidate) => candidate.id === id);
  if (found === undefined) {
    throw new Error(`Unknown story ${id}`);
  }
  return found;
}
