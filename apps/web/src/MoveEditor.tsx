import { boardCommands } from "./editor-commands";
import { copy, type Copy, type CopyKey } from "./copy";
import { useCopy } from "./preferences";
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { type PlayerSnapshot, type Ruleset } from "./public-state";
import { type CommandEnvelope, type WordRejection } from "./game-protocol";
import { useTouchInput } from "./input-capability";
import { PlayerRack } from "./PlayerRack";
import {
  draftProblem,
  editDraft,
  emptyDraft,
  reconcileDraft,
  type DraftAction,
  type MoveDraft,
} from "./move-draft";

type EditorState = {
  draft: MoveDraft;
  selected?: string;
  blank?: { tileId: string; row: number; column: number };
  message?: Copy;
  pending?: "move" | "pass" | "exchange";
  exchange?: string[];
  confirmPass?: true;
};
export function MoveEditor({
  snapshot,
  rules,
  canAct,
  onTurnAction,
  onPreview,
  wordRejection,
}: {
  snapshot: PlayerSnapshot;
  rules: Ruleset;
  canAct: boolean;
  onTurnAction: (
    command: Extract<CommandEnvelope["command"], { kind: "commit" | "pass" | "exchange" }>,
  ) => Promise<void>;
  onPreview?: (placements: MoveDraft["placements"]) => Promise<void>;
  wordRejection?: WordRejection;
}) {
  const { t } = useCopy();
  const touch = useTouchInput();
  const keyboardHelp = useId();
  const [focusSquare, setFocusSquare] = useState(
    () => Math.floor(rules.board_size / 2) * rules.board_size + Math.floor(rules.board_size / 2),
  );
  const [state, setState] = useState<EditorState>(() => ({ draft: emptyDraft(snapshot) }));
  const current = reconcileDraft(snapshot, state.draft);
  const stale = current !== state.draft;
  const selected = stale ? undefined : state.selected;
  const blank = stale ? undefined : state.blank;
  const pending = !stale && state.pending !== undefined;
  const enabled = canAct && !pending;
  const exchange = stale ? undefined : state.exchange;
  const exchanging = exchange !== undefined;
  const canExchange = snapshot.public.remaining_tiles >= rules.exchange_minimum_bag;
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const handledRejection = useRef<WordRejection>(undefined);
  useEffect(() => {
    if (!canAct && pending) {
      clearTimeout(timer.current);
      setState({
        draft: current,
        message: copy("editor.connectionChanged"),
      });
    }
  }, [canAct, pending, current]);
  useEffect(() => {
    if (wordRejection === handledRejection.current) {
      return;
    }
    handledRejection.current = wordRejection;
    if (
      wordRejection !== undefined &&
      !stale &&
      state.pending === "move" &&
      wordRejection.game_id === snapshot.public.game_id &&
      snapshot.public.phase.kind === "playing" &&
      wordRejection.expected_turn === snapshot.public.phase.turn
    ) {
      clearTimeout(timer.current);
      setState({
        draft: current,
        message: copy("editor.invalidWords", { words: wordRejection.error.words.join(", ") }),
      });
    }
  }, [wordRejection, stale, state.pending, current, snapshot.public]);
  const board = useRef<HTMLDivElement>(null);
  const editor = useRef<HTMLElement>(null);
  useEffect(() => {
    if (exchanging) {
      editor.current?.querySelector<HTMLButtonElement>(".phone-rack button")?.focus();
    }
  }, [exchanging]);
  const exchangeButton = useRef<HTMLButtonElement>(null);
  const restoreExchangeFocus = useRef(false);
  useEffect(() => {
    if (!exchanging && enabled && restoreExchangeFocus.current) {
      restoreExchangeFocus.current = false;
      exchangeButton.current?.focus();
    }
  }, [exchanging, enabled]);
  const passButton = useRef<HTMLButtonElement>(null);
  const problem = draftProblem(snapshot, rules, current);
  const previewCallback = useRef(onPreview);
  const previewPaused = useRef(false);
  useEffect(() => {
    previewCallback.current = onPreview;
  }, [onPreview]);
  const previewPlacements = JSON.stringify(current.placements);
  useEffect(() => {
    previewPaused.current = pending;
    if (!canAct || pending) {
      return;
    }
    const placements = exchanging ? [] : current.placements;
    const publish = () => {
      if (previewPaused.current) {
        return;
      }
      void previewCallback.current?.(placements).catch(() => {
        // Tentative delivery has no bearing on authoritative move acceptance.
      });
    };
    // Coalesce rapid edits; refresh only the latest complete draft.
    const initial = setTimeout(publish, 100);
    const refresh = placements.length === 0 ? undefined : setInterval(publish, 500);
    return () => {
      clearTimeout(initial);
      clearInterval(refresh);
    };
  }, [canAct, pending, current.context, previewPlacements, exchanging]);
  useEffect(() => {
    if (stale) {
      setState({ draft: current, message: copy("editor.cleared") });
    }
  }, [current, stale]);
  useEffect(() => () => clearTimeout(timer.current), [current.context]);
  useEffect(() => {
    const center = board.current?.querySelector('[data-center="true"]');
    if (center instanceof HTMLElement && board.current !== null) {
      board.current.scrollLeft =
        center.offsetLeft - board.current.clientWidth / 2 + center.offsetWidth / 2;
      board.current.scrollTop =
        center.offsetTop - board.current.clientHeight / 2 + center.offsetHeight / 2;
    }
  }, [snapshot.public.game_id]);
  function edit(action: DraftAction) {
    if (exchanging || (!enabled && action.kind !== "reset")) {
      return;
    }
    const result = editDraft(snapshot, rules, current, action);
    setState({
      draft: result.draft,
      ...(result.problem === undefined ? {} : { message: copy(`draft.${result.problem}`) }),
    });
  }
  function square(row: number, column: number) {
    if (!enabled || exchanging) {
      return;
    }
    const placed = current.placements.find(
      (tile) => tile.coordinate.row === row && tile.coordinate.column === column,
    );
    if (selected === undefined) {
      if (placed !== undefined) {
        setState({ draft: current, selected: placed.tile_id });
      }
      return;
    }
    const tile = snapshot.own_rack.tiles.find((tile) => tile.id === selected);
    if (tile?.face.kind === "blank") {
      setState({ draft: current, selected, blank: { tileId: selected, row, column } });
    } else {
      edit({ kind: "place", tileId: selected, coordinate: { row, column }, blankAs: null });
    }
  }
  async function submit(kind: "commit" | "pass" | "exchange") {
    if (
      !enabled ||
      (kind === "commit" && (problem !== undefined || current.placements.length === 0)) ||
      (kind === "pass" && state.confirmPass !== true) ||
      (kind === "exchange" &&
        (!canExchange ||
          exchange === undefined ||
          exchange.length === 0 ||
          exchange.length > snapshot.public.remaining_tiles))
    ) {
      return;
    }
    const draft = current;
    // Fence heartbeat writes before reserving the turn action's transport sequence.
    previewPaused.current = true;
    const action = kind === "commit" ? "move" : kind;
    setState({ draft, pending: action });
    timer.current = setTimeout(() => {
      setState((previous) =>
        previous.draft.context === draft.context
          ? {
              draft: previous.draft,
              message: copy(`editor.${action}Timeout`),
            }
          : previous,
      );
    }, 5000);
    try {
      if (kind === "commit") {
        await onTurnAction({ kind, placements: draft.placements });
      } else if (kind === "exchange") {
        await onTurnAction({ kind, tile_ids: exchange ?? [] });
      } else {
        await onTurnAction({ kind });
      }
    } catch {
      clearTimeout(timer.current);
      setState((previous) =>
        previous.draft.context === draft.context
          ? { draft: previous.draft, message: copy(`editor.${action}Failed`) }
          : previous,
      );
    }
  }
  const committed = new Map(
    snapshot.public.board.map((tile) => [`${tile.coordinate.row},${tile.coordinate.column}`, tile]),
  );
  const proposed = new Map(
    current.placements.map((tile) => [`${tile.coordinate.row},${tile.coordinate.column}`, tile]),
  );
  const indices = Array.from({ length: rules.board_size }, (_, index) => index);
  let feedback = state.message ?? copy(problem === undefined ? "editor.ready" : `draft.${problem}`);
  const ownTurn =
    snapshot.public.phase.kind === "playing" &&
    snapshot.public.phase.active_player === snapshot.own_rack.player_id;
  if (!ownTurn) {
    feedback = copy("draft.waiting");
  } else if (!canAct && state.message === undefined) {
    feedback = copy("editor.reconnect");
  }
  if (exchanging && enabled) {
    feedback = copy("editor.exchangeSelected", { count: exchange.length });
  }
  if (pending && state.pending !== undefined) {
    feedback = copy(`editor.${state.pending}Pending`);
  }
  function cancelSelection() {
    if (!enabled) {
      return;
    }
    setState((previous) => ({
      draft: previous.draft,
      ...(previous.exchange === undefined ? {} : { exchange: previous.exchange }),
      message: copy("editor.cancelled"),
    }));
  }
  function moveFocus(key: string, index: number): boolean {
    const command = boardCommands.find((entry) => entry.key === key);
    const next = command?.target(index, rules.board_size);
    if (next === undefined) {
      return false;
    }
    setFocusSquare(next);
    board.current?.querySelector<HTMLButtonElement>(`[data-square-index="${next}"]`)?.focus();
    return true;
  }
  return (
    <section
      ref={editor}
      className="move-editor"
      aria-label={t("editor.title")}
      data-input={touch ? "touch" : "pointer"}
      onPointerCancel={cancelSelection}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          if (pending) {
            return;
          }
          restoreExchangeFocus.current = exchanging;
          if (state.confirmPass) {
            passButton.current?.focus();
          }
          setState({ draft: current, message: copy("editor.cancelled") });
          editor.current
            ?.querySelector<HTMLButtonElement>(".phone-rack button[aria-pressed=true]")
            ?.focus();
        }
      }}
    >
      <PlayerRack
        rack={snapshot.own_rack}
        rules={rules}
        canAct={enabled}
        selection={{
          selected: exchange ?? (selected === undefined ? [] : [selected]),
          placed: current.placements.map((tile) => tile.tile_id),
          onSelect: (tileId) => {
            if (enabled) {
              setState(
                exchange === undefined
                  ? { draft: current, selected: tileId }
                  : {
                      draft: current,
                      exchange: exchange.includes(tileId)
                        ? exchange.filter((id) => id !== tileId)
                        : [...exchange, tileId],
                    },
              );
            }
          },
        }}
      />
      <p role="status" aria-live="polite">
        {t(feedback)}
      </p>
      <div
        className="phone-board"
        ref={board}
        role="region"
        tabIndex={enabled && !exchanging ? -1 : 0}
        aria-label={t("editor.board")}
      >
        <div
          className="phone-board-grid"
          role="group"
          aria-label={t("editor.chooseSquare")}
          aria-describedby={keyboardHelp}
          style={{ gridTemplateColumns: `repeat(${rules.board_size}, 44px)` }}
        >
          {indices.flatMap((row) =>
            indices.map((column) => {
              const key = `${row},${column}`;
              const fixed = committed.get(key);
              const draft = proposed.get(key);
              const rackTile = snapshot.own_rack.tiles.find((tile) => tile.id === draft?.tile_id);
              const letter =
                fixed?.letter ??
                draft?.blank_as ??
                (rackTile?.face.kind === "letter" ? rackTile.face.letter : undefined);
              const premium = rules.premiums[row * rules.board_size + column] ?? "normal";
              const center =
                row === Math.floor(rules.board_size / 2) &&
                column === Math.floor(rules.board_size / 2);
              let squareKey: CopyKey = center ? "editor.squareCenter" : "editor.squareEmpty";
              if (draft !== undefined) {
                squareKey = "editor.squareDraft";
              }
              if (fixed !== undefined) {
                squareKey = "editor.squareFixed";
              }
              const label = t(squareKey, {
                row: row + 1,
                column: column + 1,
                letter: letter ?? "",
              });
              return (
                <Button
                  key={key}
                  className={`phone-square premium-${premium.replaceAll("_", "-")}${draft ? " tentative-square" : ""}${fixed ? " committed-square" : ""}`}
                  data-center={center}
                  data-square-index={row * rules.board_size + column}
                  tabIndex={row * rules.board_size + column === focusSquare ? 0 : -1}
                  onFocus={() => setFocusSquare(row * rules.board_size + column)}
                  onKeyDown={(event: KeyboardEvent<HTMLButtonElement>) => {
                    if (moveFocus(event.key, row * rules.board_size + column)) {
                      event.preventDefault();
                    }
                  }}
                  aria-label={label}
                  aria-description={t(`premium.${premium}`)}
                  aria-pressed={draft !== undefined && selected === draft.tile_id}
                  disabled={!enabled || exchanging}
                  aria-disabled={fixed !== undefined}
                  onClick={() => {
                    if (fixed === undefined) {
                      square(row, column);
                    }
                  }}
                >
                  <small className="square-reference" aria-hidden="true">
                    {String.fromCharCode(65 + column)}
                    {row + 1}
                  </small>
                  <span aria-hidden="true">
                    {letter ?? (center ? "★" : t(`premiumShort.${premium}`) || "·")}
                  </span>
                </Button>
              );
            }),
          )}
        </div>
      </div>
      <details className={touch ? "sr-only keyboard-help" : "keyboard-help"}>
        <summary>{t("command.help")}</summary>
        <p id={keyboardHelp}>{t("editor.keyboard")}</p>
        <dl>
          {boardCommands.map((command) => (
            <div key={command.key}>
              <dt>
                <kbd>{command.key}</kbd>
              </dt>
              <dd>{t(command.label)}</dd>
            </div>
          ))}
          <div>
            <dt>
              <kbd>Enter / Space</kbd>
            </dt>
            <dd>{t("command.place")}</dd>
          </div>
          <div>
            <dt>
              <kbd>Escape</kbd>
            </dt>
            <dd>{t("command.cancel")}</dd>
          </div>
        </dl>
      </details>
      {blank !== undefined && enabled && (
        <div className="blank-choices" role="group" aria-label={t("editor.blankTitle")}>
          {rules.tiles.flatMap((tile, index) =>
            tile.face.kind === "letter"
              ? [
                  <Button
                    key={tile.face.letter}
                    autoFocus={
                      index === rules.tiles.findIndex((entry) => entry.face.kind === "letter")
                    }
                    onClick={() => {
                      edit({
                        kind: "place",
                        tileId: blank.tileId,
                        coordinate: { row: blank.row, column: blank.column },
                        blankAs: tile.face.kind === "letter" ? tile.face.letter : null,
                      });
                      board.current
                        ?.querySelector<HTMLButtonElement>(`[data-square-index="${focusSquare}"]`)
                        ?.focus();
                    }}
                  >
                    {tile.face.letter}
                  </Button>,
                ]
              : [],
          )}
          <Button
            onClick={() => {
              setState({ draft: current });
              board.current
                ?.querySelector<HTMLButtonElement>(`[data-square-index="${focusSquare}"]`)
                ?.focus();
            }}
          >
            {t("editor.cancelBlank")}
          </Button>
        </div>
      )}
      <div className="draft-actions">
        <Button
          disabled={
            !enabled ||
            exchanging ||
            selected === undefined ||
            !current.placements.some((tile) => tile.tile_id === selected)
          }
          onClick={() => selected !== undefined && edit({ kind: "remove", tileId: selected })}
        >
          {t("editor.return")}
        </Button>
        <Button
          disabled={
            pending ||
            exchanging ||
            (current.placements.length === 0 && selected === undefined && blank === undefined)
          }
          onClick={() => edit({ kind: "reset" })}
        >
          {t("editor.cancel")}
        </Button>
        <Button
          disabled={!enabled || exchanging || problem !== undefined || blank !== undefined}
          onClick={() => void submit("commit")}
        >
          {t("editor.commit")}
        </Button>
        <Button
          ref={passButton}
          disabled={!enabled || exchanging}
          onClick={() => setState({ draft: current, confirmPass: true })}
        >
          {t("editor.pass")}
        </Button>
        <Button
          ref={exchangeButton}
          disabled={!enabled || exchanging || !canExchange}
          onClick={() => setState({ draft: current, exchange: [] })}
        >
          {t("editor.exchange")}
        </Button>
      </div>
      {!canExchange && <p>{t("editor.exchangeMinimum", { count: rules.exchange_minimum_bag })}</p>}
      {exchange !== undefined && enabled && (
        <div className="pass-confirmation" role="group" aria-label={t("editor.exchange")}>
          <p>{t("editor.exchangeHelp")}</p>
          <Button
            disabled={
              exchange.length === 0 ||
              !canExchange ||
              exchange.length > snapshot.public.remaining_tiles
            }
            onClick={() => void submit("exchange")}
          >
            {t("editor.confirmExchange")}
          </Button>
          <Button
            onClick={() => {
              restoreExchangeFocus.current = true;
              setState({ draft: current });
            }}
          >
            {t("editor.keep")}
          </Button>
        </div>
      )}
      {!stale && state.confirmPass === true && enabled && (
        <div className="pass-confirmation" role="group" aria-label={t("editor.confirmPass")}>
          <p>{t("editor.passHelp")}</p>
          <Button autoFocus onClick={() => void submit("pass")}>
            {t("editor.confirmPass")}
          </Button>
          <Button
            onClick={() => {
              setState({ draft: current });
              passButton.current?.focus();
            }}
          >
            {t("editor.keep")}
          </Button>
        </div>
      )}
    </section>
  );
}
