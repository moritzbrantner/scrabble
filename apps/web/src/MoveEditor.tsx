import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState } from "react";
import { type PlayerSnapshot, type Ruleset } from "./public-state";
import { premiums } from "./board-premiums";
import { type CommandEnvelope, type WordRejection } from "./game-protocol";
import { PlayerRack } from "./PlayerRack";
import {
  draftMessages,
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
  message?: string;
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
        message: `Not in the dictionary: ${wordRejection.error.words.join(", ")}. Your draft is kept.`,
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
      setState({ draft: current, message: "The game changed. Your draft was cleared." });
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
      ...(result.problem === undefined ? {} : { message: draftMessages[result.problem] }),
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
    const label = { commit: "Move", pass: "Pass", exchange: "Exchange" }[kind];
    setState({ draft, pending: action });
    timer.current = setTimeout(() => {
      setState((previous) =>
        previous.draft.context === draft.context
          ? {
              draft: previous.draft,
              message: `${label} was not confirmed. Your draft is kept; you can retry.`,
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
          ? { draft: previous.draft, message: `Unable to send the ${action}. Your draft is kept.` }
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
  let feedback =
    state.message ??
    (problem === undefined
      ? "Ready to commit. Word acceptance is checked by the server."
      : draftMessages[problem]);
  const ownTurn =
    snapshot.public.phase.kind === "playing" &&
    snapshot.public.phase.active_player === snapshot.own_rack.player_id;
  if (!ownTurn) {
    feedback = "Wait for your turn.";
  } else if (!canAct && state.message === undefined) {
    feedback = "Reconnect to edit. Your draft is kept.";
  }
  if (exchanging && enabled) {
    feedback = `Select rack tiles to exchange. ${exchange.length} selected.`;
  }
  if (pending) {
    feedback = `Waiting for ${state.pending} confirmation…`;
  }
  return (
    <section ref={editor} className="move-editor" aria-label="Move editor">
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
        {feedback}
      </p>
      <div
        className="phone-board"
        ref={board}
        tabIndex={0}
        aria-label="Scroll the board to choose a square"
      >
        <div
          className="phone-board-grid"
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
              let description = center ? ": center" : ": empty";
              if (draft !== undefined) {
                description = `: tentative ${letter}`;
              }
              if (fixed !== undefined) {
                description = `: committed ${fixed.letter}`;
              }
              const label = `Row ${row + 1}, column ${column + 1}${description}`;
              return (
                <Button
                  key={key}
                  className={`phone-square premium-${premium.replaceAll("_", "-")}${draft ? " tentative-square" : ""}${fixed ? " committed-square" : ""}`}
                  data-center={center}
                  aria-label={label}
                  aria-description={premiums[premium].label}
                  aria-pressed={draft !== undefined && selected === draft.tile_id}
                  disabled={!enabled || exchanging || fixed !== undefined}
                  onClick={() => square(row, column)}
                >
                  <small className="square-reference" aria-hidden="true">
                    {String.fromCharCode(65 + column)}
                    {row + 1}
                  </small>
                  <span aria-hidden="true">
                    {letter ?? (center ? "★" : premiums[premium].short || "·")}
                  </span>
                </Button>
              );
            }),
          )}
        </div>
      </div>
      {blank !== undefined && enabled && (
        <div className="blank-choices" role="group" aria-label="Choose blank letter">
          {rules.tiles.flatMap((tile) =>
            tile.face.kind === "letter"
              ? [
                  <Button
                    key={tile.face.letter}
                    onClick={() =>
                      edit({
                        kind: "place",
                        tileId: blank.tileId,
                        coordinate: { row: blank.row, column: blank.column },
                        blankAs: tile.face.kind === "letter" ? tile.face.letter : null,
                      })
                    }
                  >
                    {tile.face.letter}
                  </Button>,
                ]
              : [],
          )}
          <Button onClick={() => setState({ draft: current })}>Cancel blank choice</Button>
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
          Return selected tile
        </Button>
        <Button
          disabled={
            pending ||
            exchanging ||
            (current.placements.length === 0 && selected === undefined && blank === undefined)
          }
          onClick={() => edit({ kind: "reset" })}
        >
          Cancel move
        </Button>
        <Button
          disabled={!enabled || exchanging || problem !== undefined || blank !== undefined}
          onClick={() => void submit("commit")}
        >
          Commit move
        </Button>
        <Button
          ref={passButton}
          disabled={!enabled || exchanging}
          onClick={() => setState({ draft: current, confirmPass: true })}
        >
          Pass turn
        </Button>
        <Button
          ref={exchangeButton}
          disabled={!enabled || exchanging || !canExchange}
          onClick={() => setState({ draft: current, exchange: [] })}
        >
          Exchange tiles
        </Button>
      </div>
      {!canExchange && (
        <p>Exchange requires at least {rules.exchange_minimum_bag} tiles in the bag.</p>
      )}
      {exchange !== undefined && enabled && (
        <div className="pass-confirmation" role="group" aria-label="Exchange tiles">
          <p>Choose tiles in your rack. Exchanging ends your turn and clears your draft.</p>
          <Button
            disabled={
              exchange.length === 0 ||
              !canExchange ||
              exchange.length > snapshot.public.remaining_tiles
            }
            onClick={() => void submit("exchange")}
          >
            Confirm exchange
          </Button>
          <Button
            onClick={() => {
              restoreExchangeFocus.current = true;
              setState({ draft: current });
            }}
          >
            Keep playing
          </Button>
        </div>
      )}
      {!stale && state.confirmPass === true && enabled && (
        <div className="pass-confirmation" role="group" aria-label="Confirm pass">
          <p>Pass this turn? Your draft will be cleared.</p>
          <Button autoFocus onClick={() => void submit("pass")}>
            Confirm pass
          </Button>
          <Button
            onClick={() => {
              setState({ draft: current });
              passButton.current?.focus();
            }}
          >
            Keep playing
          </Button>
        </div>
      )}
    </section>
  );
}
