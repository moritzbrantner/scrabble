import { Button, NativeSelect, NativeSelectOption } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState, type ChangeEvent } from "react";

import { AccountLink } from "./AccountLink";
import { type Copy } from "./copy";
import {
  createGame,
  CreationError,
  newCreateRequest,
  type CreateRequest,
  type JoinInformation,
} from "./create-game";
import { ClockSetup } from "./ClockSetup";
import { initialClockDraft, readTimeControl } from "./time-control";
import { BoardLayoutPreview } from "./BoardLayoutPreview";
import { ConnectedBoard } from "./ConnectedBoard";
import { CustomLayouts, type BoardChoice } from "./CustomLayouts";
import { CustomTileProfiles, type TileChoice } from "./CustomTileProfiles";
import { CustomWordLists } from "./CustomWordLists";
import { type WordChoice } from "./word-list-library";
import { reportMoenarchMatch } from "./moenarch-launch";
import { useCopy } from "./preferences";
import { fetchProfiles } from "./profile-catalog";
import { matchSettings, type MatchSettings, type ProfileCatalog } from "./profile-model";
import { faceText } from "./public-state";
import { faceClass } from "./tile-face";

const numericFields = [
  "rack_size",
  "bingo_bonus",
  "exchange_minimum_bag",
  "scoreless_turn_limit",
  "minimum_players",
  "maximum_players",
] as const;
type DraftRules = Record<(typeof numericFields)[number], string>;
function initialRules(rules: MatchSettings): DraftRules {
  return {
    rack_size: String(rules.rack_size),
    bingo_bonus: String(rules.bingo_bonus),
    exchange_minimum_bag: String(rules.exchange_minimum_bag),
    scoreless_turn_limit: String(rules.scoreless_turn_limit),
    minimum_players: String(rules.minimum_players),
    maximum_players: String(rules.maximum_players),
  };
}

type State =
  | { kind: "idle" | "creating" }
  | { kind: "failed"; message: Copy; retryable: boolean }
  | { kind: "created"; join: JoinInformation };
export function CreateBoard({
  endpoint,
  api,
  certificateHash,
  sharedBoard,
  sharedWordList,
}: {
  endpoint: string;
  api: string;
  certificateHash?: string;
  /** A shared custom layout opened through its link (#101), offered as a board choice. */
  sharedBoard?: BoardChoice;
  /** A shared custom word list opened through its link (#97), offered as a word list. */
  sharedWordList?: WordChoice;
}) {
  const { t, number } = useCopy();
  const [state, setState] = useState<State>({ kind: "idle" });
  const [catalog, setCatalog] = useState<ProfileCatalog>();
  const [catalogFailed, setCatalogFailed] = useState(false);
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  /** Bumped after account reconnect adds device capabilities, so the libraries reload. */
  const [libraryGeneration, setLibraryGeneration] = useState(0);
  const [choices, setChoices] = useState({ wordList: "", tile: "", board: "" });
  const [edits, setEdits] = useState<DraftRules>();
  const [touched, setTouched] = useState(false);
  const [clockDraft, setClockDraft] = useState(initialClockDraft);
  // Decision 1.53: dictionary hints are off for every new match unless the host opts in.
  const [dictionaryHints, setDictionaryHints] = useState(false);
  const request = useRef<CreateRequest>(undefined);
  const operation = useRef<AbortController>(undefined);
  const initialBoard = useRef(sharedBoard?.value);
  const [customBoards, setCustomBoards] = useState<BoardChoice[]>([]);
  const initialWords = useRef(sharedWordList?.value);
  const [customWords, setCustomWords] = useState<WordChoice[]>([]);
  // Saved and shared lists are selected independently of tiles and board (Decision 1.25).
  const wordChoices: WordChoice[] = [
    ...(catalog?.wordLists.map((row) => ({
      value: row.identity.name,
      label: row.label,
      identity: row.identity,
    })) ?? []),
    ...customWords,
    ...(sharedWordList ? [sharedWordList] : []),
  ];
  // A deleted saved list falls back to the catalog default selection.
  const selectedWords =
    wordChoices.find((choice) => choice.value === choices.wordList) ??
    wordChoices.find((choice) => choice.value === catalog?.defaults.selection.wordList.name);
  const customWordsSelected =
    selectedWords !== undefined &&
    !catalog?.wordLists.some((row) => row.identity.name === selectedWords.value);
  const boardChoices: BoardChoice[] = [
    ...(catalog?.boardProfiles.map((row) => ({ value: row.identity.name, row })) ?? []),
    ...customBoards,
    ...(sharedBoard ? [sharedBoard] : []),
  ];
  // A deleted custom layout falls back to the catalog default selection.
  const selectedChoice =
    boardChoices.find((choice) => choice.value === choices.board) ??
    boardChoices.find((choice) => choice.value === catalog?.defaults.selection.board.name);
  const selectedBoard = selectedChoice?.row;
  const customSelected =
    selectedChoice !== undefined &&
    !catalog?.boardProfiles.some((row) => row.identity.name === selectedChoice.value);
  const [customTiles, setCustomTiles] = useState<TileChoice[]>([]);
  // Saved private tile profiles are selected independently of words and board (Decision 1.25).
  const tileChoices: TileChoice[] = [
    ...(catalog?.tileProfiles.map((row) => ({ value: row.identity.name, row })) ?? []),
    ...customTiles,
  ];
  // A deleted saved profile falls back to the catalog default selection.
  const selectedTileChoice =
    tileChoices.find((choice) => choice.value === choices.tile) ??
    tileChoices.find((choice) => choice.value === catalog?.defaults.selection.tile.name);
  const selectedTiles = selectedTileChoice?.row;
  const customTilesSelected =
    selectedTileChoice !== undefined &&
    !catalog?.tileProfiles.some((row) => row.identity.name === selectedTileChoice.value);
  const boardSizes = catalog
    ? [...new Set(catalog.boardProfiles.map((row) => row.boardSize))].sort((a, b) => a - b)
    : [];
  useEffect(() => () => operation.current?.abort(), []);
  useEffect(() => {
    const controller = new AbortController();
    setCatalogFailed(false);
    void fetchProfiles(api, controller.signal)
      .then((value) => {
        if (controller.signal.aborted) {
          return;
        }
        setCatalog(value);
        setChoices({
          wordList: initialWords.current ?? value.defaults.selection.wordList.name,
          tile: value.defaults.selection.tile.name,
          board: initialBoard.current ?? value.defaults.selection.board.name,
        });
        setEdits(initialRules(value.defaults.rules));
      })
      .catch(() => {
        if (!controller.signal.aborted) {
          setCatalogFailed(true);
        }
      });
    return () => controller.abort();
  }, [api, catalogAttempt]);
  function change() {
    setTouched(true);
    request.current = undefined;
    setState({ kind: "idle" });
  }

  async function create(fresh: boolean) {
    if (operation.current !== undefined && !operation.current.signal.aborted) {
      return;
    }
    const controller = new AbortController();
    operation.current = controller;
    setState({ kind: "creating" });
    try {
      if (fresh || request.current === undefined) {
        const intent = newCreateRequest();
        if (!catalog || !edits) {
          throw new CreationError("profiles.unavailable", false);
        }
        const clock = readTimeControl(clockDraft);
        if (clock.error) {
          throw new CreationError(clock.error, false);
        }
        if (
          !touched &&
          !customSelected &&
          !customWordsSelected &&
          !customTilesSelected &&
          catalog.defaults.legacyCompatible &&
          clock.control === undefined &&
          !dictionaryHints
        ) {
          request.current = intent;
        } else {
          const rules = matchSettings.safeParse({
            ...Object.fromEntries(
              numericFields.map((key) => [
                key,
                edits[key].trim() === "" ? NaN : Number(edits[key]),
              ]),
            ),
            ...(dictionaryHints ? { dictionary_hints: true } : {}),
          });
          const wordList = selectedWords?.identity;
          const wordListAccess = selectedWords?.access;
          const tile = selectedTiles?.identity;
          const board = selectedBoard?.identity;
          if (!rules.success) {
            throw new CreationError("profiles.invalidRules", false);
          }
          if (!wordList || !tile || !board) {
            throw new CreationError("profiles.unknown", false);
          }
          request.current = {
            ...intent,
            version: 3,
            selection: { wordList, tile, board, ...(wordListAccess ? { wordListAccess } : {}) },
            rules: rules.data,
            ...(clock.control === undefined ? {} : { timeControl: clock.control }),
          };
        }
      }
      const join = await createGame(api, endpoint, request.current, controller.signal);
      if (!controller.signal.aborted) {
        const location = new URL(window.location.href);
        location.searchParams.set("match", join.matchId);
        window.history.replaceState(null, "", location);
        setState({ kind: "created", join });
      }
    } catch (error) {
      if (!controller.signal.aborted) {
        const failure =
          error instanceof CreationError ? error : new CreationError("create.failed", false);
        setState({ kind: "failed", message: failure.copy, retryable: failure.retryable });
      }
    } finally {
      controller.abort();
    }
  }
  if (state.kind === "created") {
    return (
      <>
        <ConnectedBoard
          key={state.join.matchId}
          onBoardOwned={reportMoenarchMatch}
          api={api}
          onRematch={(join, rematchRequest) => {
            request.current = rematchRequest;
            const location = new URL(window.location.href);
            location.searchParams.delete("launch");
            location.searchParams.delete("session");
            location.searchParams.set("match", join.matchId);
            window.history.replaceState(null, "", location);
            setState({ kind: "created", join });
          }}
          endpoint={endpoint}
          matchId={state.join.matchId}
          {...(request.current === undefined
            ? {}
            : { boardClaim: { requestId: request.current.requestId, gameId: state.join.gameId } })}
          {...(certificateHash === undefined ? {} : { certificateHash })}
        />
      </>
    );
  }
  return (
    <main className="create-board">
      <h1>Scrabble</h1>
      {catalog === undefined && !catalogFailed && <p role="status">{t("profiles.loading")}</p>}
      {catalogFailed && (
        <>
          <p role="alert">{t("profiles.unavailable")}</p>
          <Button onClick={() => setCatalogAttempt((value) => value + 1)}>
            {t("profiles.retry")}
          </Button>
        </>
      )}
      {catalog && edits && (
        <fieldset className="match-setup" disabled={state.kind === "creating"}>
          <div className="match-setup-columns">
            <div className="match-setup-controls">
              <div className="match-profile-fields">
                <label>
                  {t("profiles.wordList")}
                  <NativeSelect
                    aria-label={t("profiles.wordList")}
                    value={selectedWords?.value ?? ""}
                    onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                      change();
                      setChoices({ ...choices, wordList: event.currentTarget.value });
                    }}
                  >
                    {wordChoices.map((choice) => (
                      <NativeSelectOption key={choice.value} value={choice.value}>
                        {choice.label}
                      </NativeSelectOption>
                    ))}
                  </NativeSelect>
                  <small className="setting-note">{t("profiles.wordListExplanation")}</small>
                </label>
                <label>
                  {t("profiles.tiles")}
                  <NativeSelect
                    aria-label={t("profiles.tiles")}
                    value={selectedTileChoice?.value ?? ""}
                    onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                      change();
                      setChoices({ ...choices, tile: event.currentTarget.value });
                    }}
                  >
                    {tileChoices.map((choice) => (
                      <NativeSelectOption key={choice.value} value={choice.value}>
                        {choice.row.label}
                      </NativeSelectOption>
                    ))}
                  </NativeSelect>
                  <small className="setting-note">{t("profiles.tilesExplanation")}</small>
                </label>
                {boardSizes.length > 1 && (
                  <label>
                    {t("profiles.boardSize")}
                    <NativeSelect
                      aria-label={t("profiles.boardSize")}
                      value={String(selectedBoard?.boardSize ?? "")}
                      onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                        const size = Number(event.currentTarget.value);
                        const board = catalog.boardProfiles.find((row) => row.boardSize === size);
                        if (board) {
                          change();
                          setChoices({ ...choices, board: board.identity.name });
                        }
                      }}
                    >
                      {boardSizes.map((size) => (
                        <NativeSelectOption key={size} value={String(size)}>
                          {t("profiles.squareBoard", { size: number(size) })}
                        </NativeSelectOption>
                      ))}
                    </NativeSelect>
                    <small className="setting-note">{t("profiles.boardSizeExplanation")}</small>
                  </label>
                )}
                <label>
                  {t("profiles.board")}
                  <NativeSelect
                    aria-label={t("profiles.board")}
                    value={selectedChoice?.value ?? ""}
                    onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                      change();
                      setChoices({ ...choices, board: event.currentTarget.value });
                    }}
                  >
                    {boardChoices.map((choice) => (
                      <NativeSelectOption key={choice.value} value={choice.value}>
                        {choice.row.label}
                      </NativeSelectOption>
                    ))}
                  </NativeSelect>
                  <small className="setting-note">{t("profiles.boardExplanation")}</small>
                </label>
              </div>
              {selectedTiles && (
                <details className="tile-profile-summary">
                  <summary>{t("profiles.distribution")}</summary>
                  <p className="setting-note">{t("profiles.distributionExplanation")}</p>
                  <ul className="tile-preview-list">
                    {selectedTiles.tiles.map((tile) => {
                      const face = faceText(tile.face) ?? "□";
                      return (
                        <li key={face}>
                          <span className="tile-preview-square">
                            <strong className={faceClass(face)}>{face}</strong>
                            <small>{number(tile.value)}</small>
                          </span>
                          <span className="tile-preview-count">×{number(tile.count)}</span>
                        </li>
                      );
                    })}
                  </ul>
                </details>
              )}
              <ClockSetup
                draft={clockDraft}
                onChange={(draft) => {
                  change();
                  setClockDraft(draft);
                }}
              />
              <label className="dictionary-hints">
                <input
                  type="checkbox"
                  checked={dictionaryHints}
                  onChange={(event: ChangeEvent<HTMLInputElement>) => {
                    change();
                    setDictionaryHints(event.currentTarget.checked);
                  }}
                />{" "}
                {t("create.dictionaryHints")}
                <small className="setting-note">{t("create.dictionaryHintsHelp")}</small>
              </label>
              <details className="advanced-rules">
                <summary>{t("profiles.advanced")}</summary>
                <div className="advanced-match-rules">
                  {numericFields.map((key) => (
                    <label key={key}>
                      {t(`profiles.${key}`)}
                      <input
                        type="number"
                        aria-label={t(`profiles.${key}`)}
                        step="1"
                        value={edits[key]}
                        onChange={(event: ChangeEvent<HTMLInputElement>) => {
                          change();
                          setEdits({ ...edits, [key]: event.currentTarget.value });
                        }}
                      />
                      <small className="setting-note">{t(`profiles.${key}Explanation`)}</small>
                    </label>
                  ))}
                </div>
              </details>
            </div>
            {selectedBoard && <BoardLayoutPreview board={selectedBoard} />}
          </div>
        </fieldset>
      )}

      {state.kind === "creating" && <p role="status">{t("create.creating")}</p>}
      {state.kind === "failed" && <p role="alert">{t(state.message)}</p>}
      <Button
        disabled={state.kind === "creating" || catalog === undefined}
        onClick={() => void create(state.kind !== "failed" || !state.retryable)}
      >
        {state.kind === "failed" && state.retryable ? t("create.retry") : t("create.action")}
      </Button>
      <AccountLink api={api} onReconnected={() => setLibraryGeneration((value) => value + 1)} />
      <CustomWordLists key={`words-${libraryGeneration}`} api={api} onChoices={setCustomWords} />
      <CustomTileProfiles
        key={`tiles-${libraryGeneration}`}
        api={api}
        template={selectedTiles}
        onChoices={setCustomTiles}
      />
      <CustomLayouts key={`boards-${libraryGeneration}`} api={api} onChoices={setCustomBoards} />
    </main>
  );
}
