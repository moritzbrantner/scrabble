/**
 * Independent expected UI-state contract for the Scrabble multiplayer journey (#186,
 * ADR 0012, Decision Atlas 6.1–6.3). This file is reviewed by hand. It deliberately
 * imports nothing from the application: expected states, transitions and observable
 * outcomes are declared here and only *checked* against the real browsers and the
 * authoritative Rust server. Observed behaviour never rewrites it.
 *
 * Views: `board` is the public shared board (player 1), `ada` the first joined phone
 * (player 2, moves first in the fixture opening) and `lin` the second (player 3).
 */

export type View = "board" | "ada" | "lin";
export type Seat = "ada" | "lin";

/** Observable outcomes that identify a state. Unset fields are not checked. */
export type Expected = {
  /** Authoritative phase kind in the shared board's wire snapshot of the created match. */
  phase?: "lobby" | "playing" | "finished";
  /** Visible buttons each view must offer. */
  buttons?: Partial<Record<View, string>>;
  /** Seated players listed on the shared board's roster. */
  seated?: number;
  /** Active player and turn number while playing. */
  active?: Seat;
  turn?: number;
  /** Committed tiles on the authoritative board. */
  boardTiles?: number;
  /** Tentative squares in Ada's private move editor. */
  adaDraft?: number;
  /** Tiles in Ada's private proposed placement on her own wire view. */
  adaPreview?: number;
  /** Ada's transport: a held reconnect shows "Restoring game…". */
  adaConnection?: "restoring" | "reconnected";
  /** The board shows an invite for a different match than the one it created. */
  rematchInvite?: boolean;
  /** Visible text each view must show. */
  text?: Partial<Record<View, string>>;
};

export type StateId =
  | "setup"
  | "lobby"
  | "ada-joined"
  | "ready"
  | "started"
  | "draft-rejected"
  | "proposed"
  | "restoring"
  | "reconnected"
  | "committed"
  | "passed"
  | "finished"
  | "rematch-lobby";

export type State = { id: StateId; label: string; expected: Expected | null };

export type TransitionId =
  | "create-game"
  | "reject-malformed-invite"
  | "ada-joins"
  | "lin-joins"
  | "start-game"
  | "commit-rejected-word"
  | "cancel-draft"
  | "propose-at"
  | "commit-at"
  | "interrupt-ada"
  | "release-snapshot"
  | "commit-after-reconnect"
  | "lin-passes"
  | "pass-out"
  | "start-rematch";

export type Transition = {
  id: TransitionId;
  from: StateId;
  to: StateId;
  /** The real user action, in reviewer words. */
  action: string;
  /** Negative edges are modeled rejections: the action is refused and reported. */
  kind: "positive" | "negative";
  /** Rejection shown to the acting view, checked in addition to the target state. */
  rejection?: { view: View; text: string };
};

/** Every match starts from the shared board's creation page. */
export const initialState: StateId = "setup";

export const states: readonly State[] = [
  { id: "setup", label: "Board on the create page", expected: null },
  {
    id: "lobby",
    label: "Lobby created, nobody seated",
    expected: { phase: "lobby", seated: 0, boardTiles: 0 },
  },
  {
    id: "ada-joined",
    label: "Ada seated, waiting for players",
    expected: { phase: "lobby", seated: 1, text: { ada: "Ada" } },
  },
  {
    id: "ready",
    label: "Ada and Lin seated",
    expected: { phase: "lobby", seated: 2, text: { lin: "Lin" } },
  },
  {
    id: "started",
    label: "Ada's turn, no draft",
    expected: {
      phase: "playing",
      active: "ada",
      turn: 0,
      boardTiles: 0,
      adaDraft: 0,
      text: { ada: "It is your turn." },
    },
  },
  {
    id: "draft-rejected",
    label: "Ada's TA commit rejected, draft kept",
    expected: {
      phase: "playing",
      active: "ada",
      turn: 0,
      boardTiles: 0,
      adaDraft: 2,
      text: { ada: "Not in the dictionary:" },
    },
  },
  {
    id: "proposed",
    label: "Ada proposes AT",
    expected: {
      phase: "playing",
      active: "ada",
      turn: 0,
      boardTiles: 0,
      adaDraft: 2,
      adaPreview: 2,
      text: { ada: "Ready to commit" },
    },
  },
  {
    id: "restoring",
    label: "Ada disconnected, restoring",
    expected: {
      phase: "playing",
      active: "ada",
      turn: 0,
      boardTiles: 0,
      adaDraft: 2,
      adaConnection: "restoring",
      text: { ada: "Restoring game…" },
    },
  },
  {
    id: "reconnected",
    label: "Ada reconnected, draft kept",
    expected: {
      phase: "playing",
      active: "ada",
      turn: 0,
      boardTiles: 0,
      adaDraft: 2,
      adaConnection: "reconnected",
      text: { ada: "Connected · Player 2" },
    },
  },
  {
    id: "committed",
    label: "AT committed, Lin's turn",
    expected: { phase: "playing", active: "lin", turn: 1, boardTiles: 2, adaDraft: 0 },
  },
  {
    id: "passed",
    label: "Lin passed, Ada's turn",
    expected: { phase: "playing", active: "ada", turn: 2, boardTiles: 2 },
  },
  {
    id: "finished",
    label: "Finished after six passes",
    expected: {
      phase: "finished",
      boardTiles: 0,
      text: { board: "Game finished", ada: "Game finished.", lin: "Game finished." },
    },
  },
  {
    id: "rematch-lobby",
    label: "Rematch lobby on the board",
    // The board's observed wire seat belongs to the finished match, so the rematch
    // lobby is identified from its visible invite, roster and Start control.
    expected: { seated: 0, rematchInvite: true, buttons: { board: "Start game" } },
  },
];

export const transitions: readonly Transition[] = [
  {
    id: "create-game",
    from: "setup",
    to: "lobby",
    action: "Board: Create game",
    kind: "positive",
  },
  {
    id: "reject-malformed-invite",
    from: "lobby",
    to: "lobby",
    action: "Ada opens an invite with a malformed game identifier",
    kind: "negative",
    rejection: { view: "ada", text: "invalid game identifier" },
  },
  {
    id: "ada-joins",
    from: "lobby",
    to: "ada-joined",
    action: "Ada opens the invite and joins",
    kind: "positive",
  },
  {
    id: "lin-joins",
    from: "ada-joined",
    to: "ready",
    action: "Lin opens the invite and joins",
    kind: "positive",
  },
  {
    id: "start-game",
    from: "ready",
    to: "started",
    action: "Board: Start game",
    kind: "positive",
  },
  {
    id: "commit-rejected-word",
    from: "started",
    to: "draft-rejected",
    action: "Ada places TA and commits",
    kind: "negative",
    rejection: { view: "ada", text: "Not in the dictionary:" },
  },
  {
    id: "cancel-draft",
    from: "draft-rejected",
    to: "started",
    action: "Ada: Cancel move",
    kind: "positive",
  },
  {
    id: "propose-at",
    from: "started",
    to: "proposed",
    action: "Ada places A and T on the centre row",
    kind: "positive",
  },
  {
    id: "commit-at",
    from: "proposed",
    to: "committed",
    action: "Ada: Commit move",
    kind: "positive",
  },
  {
    id: "interrupt-ada",
    from: "proposed",
    to: "restoring",
    action: "Ada's connection drops (real QUIC close)",
    kind: "positive",
  },
  {
    id: "release-snapshot",
    from: "restoring",
    to: "reconnected",
    action: "Ada's restored snapshot arrives",
    kind: "positive",
  },
  {
    id: "commit-after-reconnect",
    from: "reconnected",
    to: "committed",
    action: "Ada: Commit move after reconnecting",
    kind: "positive",
  },
  {
    id: "lin-passes",
    from: "committed",
    to: "passed",
    action: "Lin: Pass turn, Confirm pass",
    kind: "positive",
  },
  {
    id: "pass-out",
    from: "started",
    to: "finished",
    action: "Ada and Lin confirm six passes (scoreless-turn limit)",
    kind: "positive",
  },
  {
    id: "start-rematch",
    from: "finished",
    to: "rematch-lobby",
    action: "Board: Start rematch",
    kind: "positive",
  },
];

/**
 * Intentionally unmodeled controls for the v1 graph (owner answer Q3, 2026-10-22).
 * Control discovery (PR2) reports anything else as a review candidate.
 */
export const exclusions: readonly { area: string; reason: string }[] = [
  { area: "Match clocks", reason: "Timed play is outside the v1 journey." },
  { area: "Hexagonal boards", reason: "Board-geometry variants are outside the v1 journey." },
  { area: "Custom board layouts", reason: "Layout authoring is outside the v1 journey." },
  { area: "Settings (language, appearance)", reason: "Covered by ui-state-contract.spec.ts." },
  { area: "Pre-move", reason: "Waiting-player drafts are outside the v1 journey." },
];
