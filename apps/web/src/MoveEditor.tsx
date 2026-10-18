import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useId, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

import { copy, type Copy, type CopyKey } from "./copy";
import { boardCells, containsCell, hexCellStyle, hexBoardStyle } from "./board-geometry";
import { draftEvaluation } from "./hex-preview";
import { MoveAnalysis } from "./MoveAnalysis";
import { boardCommands } from "./editor-commands";
import { type CommandEnvelope, type TurnRejection } from "./game-protocol";
import { useTouchInput } from "./input-capability";
import {
  draftProblem,
  editDraft,
  emptyDraft,
  persistDraft,
  reconcile,
  restoreDraft,
  type DraftAction,
  type MoveDraft,
} from "./move-draft";
import { PlayerRack } from "./PlayerRack";
import { arrangeRack, moveInOrder } from "./rack-order";
import { useCopy } from "./preferences";
import { faceText, type PlayerSnapshot, type Ruleset } from "./public-state";
import { faceClass } from "./tile-face";

type EditorState = {
  draft: MoveDraft;
  selected?: string;
  blank?: { tileId: string; row: number; column: number };
  message?: Copy;
  pending?: "move" | "pass" | "exchange";
  exchange?: string[];
  confirmPass?: true;
};
type Drag = {
  tileId: string;
  from: "rack" | "board";
  pointerId: number;
  x: number;
  y: number;
  active: boolean;
};
/** Pointer travel before a press becomes a drag; shorter presses stay taps. */
const DRAG_THRESHOLD_PX = 8;
export function MoveEditor({
  snapshot,
  rules,
  canAct,
  canPrepare = false,
  persistKey,
  onTurnAction,
  onPreview,
  turnRejection,
}: {
  snapshot: PlayerSnapshot;
  rules: Ruleset;
  canAct: boolean;
  /** A connected seated player may prepare a private pre-move while waiting (Decision 2.8). */
  canPrepare?: boolean;
  /** Same-device persistence of the single pre-move (Decision 2.10); omitted in harnesses. */
  persistKey?: string;
  onTurnAction: (
    command: Extract<CommandEnvelope["command"], { kind: "commit" | "pass" | "exchange" }>,
  ) => Promise<void>;
  onPreview?: (placements: MoveDraft["placements"]) => Promise<void>;
  turnRejection?: TurnRejection;
}) {
  const { t } = useCopy();
  const touch = useTouchInput();
  const keyboardHelp = useId();
  const [focusSquare, setFocusSquare] = useState(
    () => Math.floor(rules.board_size / 2) * rules.board_size + Math.floor(rules.board_size / 2),
  );
  const [state, setState] = useState<EditorState>(() => ({
    draft: restoreDraft(persistKey, snapshot, rules) ?? emptyDraft(snapshot),
  }));
  // Physical rack arrangement is player-local presentation, never authoritative state.
  const [rackOrder, setRackOrder] = useState<string[]>([]);
  const rackTiles = arrangeRack(snapshot.own_rack.tiles, rackOrder);
  const rackIds = rackTiles.map((tile) => tile.id);
  const [dragging, setDragging] = useState<string>();
  const drag = useRef<Drag>(undefined);
  const ghost = useRef<HTMLDivElement>(null);
  const suppressClick = useRef(false);
  const dropRef = useRef<(drag: Drag, x: number, y: number) => void>(() => {});
  const reconciled = reconcile(snapshot, state.draft);
  const current = reconciled.draft;
  const stale = current !== state.draft;
  const selected = stale ? undefined : state.selected;
  const blank = stale ? undefined : state.blank;
  const pending = !stale && state.pending !== undefined;
  const enabled = canAct && !pending;
  const editable = (canAct || canPrepare) && !pending;
  const exchange = stale ? undefined : state.exchange;
  const exchanging = exchange !== undefined;
  const canExchange = snapshot.public.remaining_tiles >= rules.exchange_minimum_bag;
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const handledRejection = useRef<TurnRejection>(undefined);
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
    if (turnRejection === handledRejection.current) {
      return;
    }
    handledRejection.current = turnRejection;
    if (
      turnRejection !== undefined &&
      !stale &&
      state.pending === (turnRejection.error.kind === "invalid_words" ? "move" : "exchange") &&
      turnRejection.game_id === snapshot.public.game_id &&
      snapshot.public.phase.kind === "playing" &&
      turnRejection.expected_turn === snapshot.public.phase.turn
    ) {
      clearTimeout(timer.current);
      setState({
        draft: current,
        message:
          turnRejection.error.kind === "invalid_words"
            ? copy("editor.invalidWords", { words: turnRejection.error.words.join(", ") })
            : copy("profiles.exchangeUnavailable"),
      });
    }
  }, [turnRejection, stale, state.pending, current, snapshot.public]);
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
  const evaluation = canAct ? draftEvaluation(snapshot, current) : undefined;
  const awaitingAnalysis = rules.board_shape === "hexagon" && evaluation?.status !== "valid";
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
      const reset = reconciled.reset;
      setState(
        reset === undefined
          ? { draft: current }
          : {
              draft: current,
              message: copy(reset === "overlap" ? "editor.premoveOverlap" : "editor.cleared"),
            },
      );
    }
  }, [current, stale, reconciled.reset]);
  useEffect(() => {
    persistDraft(persistKey, state.draft);
  }, [persistKey, state.draft]);
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
    if (exchanging || (!editable && action.kind !== "reset")) {
      return;
    }
    const result = editDraft(snapshot, rules, current, action);
    setState({
      draft: result.draft,
      ...(result.problem === undefined ? {} : { message: copy(`draft.${result.problem}`) }),
    });
  }
  function square(row: number, column: number) {
    if (!editable || exchanging) {
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
      (kind === "commit" &&
        (problem !== undefined || awaitingAnalysis || current.placements.length === 0)) ||
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
  function drop(source: Drag, x: number, y: number) {
    const target = document.elementFromPoint(x, y);
    const squareElement = target?.closest<HTMLElement>("[data-square-index]");
    if (squareElement !== null && squareElement !== undefined) {
      if (!editor.current?.contains(squareElement)) {
        return;
      }
      const index = Number(squareElement.dataset.squareIndex);
      const row = Math.floor(index / rules.board_size);
      const column = index % rules.board_size;
      setFocusSquare(index);
      if (committed.has(`${row},${column}`)) {
        setState({ draft: current, message: copy("draft.occupied") });
        return;
      }
      const existing = current.placements.find((tile) => tile.tile_id === source.tileId);
      const tile = snapshot.own_rack.tiles.find((tile) => tile.id === source.tileId);
      if (tile?.face.kind === "blank" && (existing?.blank_as ?? null) === null) {
        setState({
          draft: current,
          selected: source.tileId,
          blank: { tileId: source.tileId, row, column },
        });
      } else {
        edit({
          kind: "place",
          tileId: source.tileId,
          coordinate: { row, column },
          blankAs: existing?.blank_as ?? null,
        });
      }
      return;
    }
    const rackItem = target?.closest<HTMLElement>(".phone-rack li[data-tile-id]");
    if (!target?.closest(".phone-rack") || !editor.current?.contains(target)) {
      return;
    }
    if (source.from === "board") {
      edit({ kind: "remove", tileId: source.tileId });
    }
    const targetId = rackItem?.dataset.tileId;
    if (targetId !== source.tileId) {
      setRackOrder(
        moveInOrder(
          rackIds,
          source.tileId,
          targetId === undefined ? rackIds.length : rackIds.indexOf(targetId),
        ),
      );
    }
  }
  useEffect(() => {
    dropRef.current = drop;
  });
  function beginDrag(tileId: string, from: Drag["from"], event: PointerEvent<HTMLElement>) {
    if (!editable || exchanging || drag.current !== undefined || event.button !== 0) {
      return;
    }
    drag.current = {
      tileId,
      from,
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      active: false,
    };
    const move = (moved: globalThis.PointerEvent) => {
      const source = drag.current;
      if (source === undefined || moved.pointerId !== source.pointerId) {
        return;
      }
      if (!source.active) {
        if (Math.hypot(moved.clientX - source.x, moved.clientY - source.y) < DRAG_THRESHOLD_PX) {
          return;
        }
        source.active = true;
        setDragging(source.tileId);
      }
      moved.preventDefault();
      ghost.current?.style.setProperty(
        "transform",
        `translate(${moved.clientX}px, ${moved.clientY}px) translate(-50%, -50%)`,
      );
    };
    const finish = (ended: globalThis.PointerEvent) => {
      const source = drag.current;
      if (source === undefined || ended.pointerId !== source.pointerId) {
        return;
      }
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      drag.current = undefined;
      if (!source.active) {
        return;
      }
      setDragging(undefined);
      // The release must not also count as a tap on whatever lies beneath it.
      suppressClick.current = true;
      setTimeout(() => {
        suppressClick.current = false;
      }, 0);
      if (ended.type === "pointerup") {
        dropRef.current(source, ended.clientX, ended.clientY);
      } else {
        setState((previous) => ({
          draft: previous.draft,
          message: copy("editor.dragCancelled"),
        }));
      }
    };
    window.addEventListener("pointermove", move, { passive: false });
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
  }
  function rackKey(tileId: string, event: KeyboardEvent<HTMLButtonElement>) {
    const step = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[event.key];
    if (!event.altKey || step === undefined || exchanging) {
      return;
    }
    event.preventDefault();
    const position = Math.max(0, Math.min(rackIds.indexOf(tileId) + step, rackIds.length - 1));
    setRackOrder(moveInOrder(rackIds, tileId, position));
    setState({
      draft: current,
      ...(selected === undefined ? {} : { selected }),
      message: copy("editor.rackMoved", { position: position + 1, count: rackIds.length }),
    });
    requestAnimationFrame(() =>
      editor.current
        ?.querySelector<HTMLButtonElement>(`.phone-rack li[data-tile-id="${tileId}"] button`)
        ?.focus(),
    );
  }
  const draggedTile = snapshot.own_rack.tiles.find((tile) => tile.id === dragging);
  const draggedPlacement = current.placements.find((tile) => tile.tile_id === dragging);
  const draggedLetter =
    draggedPlacement?.blank_as ??
    (draggedTile === undefined ? undefined : (faceText(draggedTile.face) ?? "◇"));

  let feedback = state.message ?? copy(problem === undefined ? "editor.ready" : `draft.${problem}`);
  const ownTurn =
    snapshot.public.phase.kind === "playing" &&
    snapshot.public.phase.active_player === snapshot.own_rack.player_id;
  if (!ownTurn) {
    feedback = copy(current.placements.length > 0 && editable ? "draft.premove" : "draft.waiting");
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
    if (!editable) {
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
    if (
      next === undefined ||
      !containsCell(rules, Math.floor(next / rules.board_size), next % rules.board_size)
    ) {
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
        rack={{ ...snapshot.own_rack, tiles: rackTiles }}
        rules={rules}
        canAct={editable}
        selection={{
          selected: exchange ?? (selected === undefined ? [] : [selected]),
          placed: current.placements.map((tile) => tile.tile_id),
          ...(dragging === undefined ? {} : { dragging }),
          onPointerDown: (tileId, event) => beginDrag(tileId, "rack", event),
          onKeyDown: rackKey,
          onSelect: (tileId) => {
            if (editable && !suppressClick.current) {
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
        tabIndex={editable && !exchanging ? -1 : 0}
        aria-label={t("editor.board")}
      >
        <div
          className={`phone-board-grid${rules.board_shape === "hexagon" ? " hex-editor-grid" : ""}`}
          role="group"
          aria-label={t("editor.chooseSquare")}
          aria-describedby={keyboardHelp}
          style={
            rules.board_shape === "hexagon"
              ? { ...hexBoardStyle(rules.board_size), maxWidth: "none" }
              : { gridTemplateColumns: `repeat(${rules.board_size}, 44px)` }
          }
        >
          {boardCells(rules).map(({ row, column, premiumIndex }) => {
            const key = `${row},${column}`;
            const fixed = committed.get(key);
            const draft = proposed.get(key);
            const rackTile = snapshot.own_rack.tiles.find((tile) => tile.id === draft?.tile_id);
            const letter =
              fixed?.letter ??
              draft?.blank_as ??
              (rackTile === undefined ? undefined : (faceText(rackTile.face) ?? undefined));
            const premium = rules.premiums[premiumIndex] ?? "normal";
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
                style={
                  rules.board_shape === "hexagon"
                    ? hexCellStyle(rules.board_size, row, column)
                    : undefined
                }
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
                disabled={!editable || exchanging}
                aria-disabled={fixed !== undefined}
                data-dragging={draft !== undefined && dragging === draft.tile_id ? true : undefined}
                onPointerDown={(event: PointerEvent<HTMLButtonElement>) => {
                  if (draft !== undefined) {
                    beginDrag(draft.tile_id, "board", event);
                  }
                }}
                onClick={() => {
                  if (fixed === undefined && !suppressClick.current) {
                    square(row, column);
                  }
                }}
              >
                <small className="square-reference" aria-hidden="true">
                  {String.fromCharCode(65 + column)}
                  {row + 1}
                </small>
                <span aria-hidden="true" className={letter && faceClass(letter)}>
                  {letter ?? (center ? "★" : t(`premiumShort.${premium}`) || "·")}
                </span>
              </Button>
            );
          })}
        </div>
      </div>
      {evaluation?.status === "valid" && <MoveAnalysis analysis={evaluation.analysis} preview />}
      {evaluation?.status === "invalid" && (
        <p role="status">
          {t(`hex.error.${evaluation.problem}`, { words: evaluation.words.join(", ") })}
        </p>
      )}
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
          <div>
            <dt>
              <kbd>Alt + ← / →</kbd>
            </dt>
            <dd>{t("command.rackMove")}</dd>
          </div>
        </dl>
      </details>
      <div ref={ghost} className="drag-ghost rack-tile" aria-hidden="true" hidden={!draggedLetter}>
        {draggedLetter !== undefined && (
          <span className={faceClass(draggedLetter)}>{draggedLetter}</span>
        )}
      </div>
      {blank !== undefined && editable && (
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
            !editable ||
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
          disabled={
            !enabled ||
            exchanging ||
            problem !== undefined ||
            awaitingAnalysis ||
            blank !== undefined
          }
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
