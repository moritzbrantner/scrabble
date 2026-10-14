import { Button, NativeSelect, NativeSelectOption } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState, type ChangeEvent } from "react";

import { type Copy } from "./copy";
import {
  createGame,
  CreationError,
  newCreateRequest,
  type CreateRequest,
  type JoinInformation,
} from "./create-game";
import { LiveBoard } from "./LiveBoard";
import { reportMoenarchMatch } from "./moenarch-launch";
import { useCopy } from "./preferences";
import { fetchProfiles } from "./profile-catalog";
import { matchSettings, type MatchSettings, type ProfileCatalog } from "./profile-model";

const numericFields = [
  "rack_size",
  "bingo_bonus",
  "exchange_minimum_bag",
  "scoreless_turn_limit",
  "minimum_players",
  "maximum_players",
] as const;
type DraftRules = Record<keyof MatchSettings, string>;
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
}: {
  endpoint: string;
  api: string;
  certificateHash?: string;
}) {
  const { t } = useCopy();
  const [state, setState] = useState<State>({ kind: "idle" });
  const [catalog, setCatalog] = useState<ProfileCatalog>();
  const [catalogFailed, setCatalogFailed] = useState(false);
  const [catalogAttempt, setCatalogAttempt] = useState(0);
  const [choices, setChoices] = useState({ wordList: "", tile: "", board: "" });
  const [edits, setEdits] = useState<DraftRules>();
  const [touched, setTouched] = useState(false);
  const request = useRef<CreateRequest>(undefined);
  const operation = useRef<AbortController>(undefined);
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
          wordList: value.defaults.selection.wordList.name,
          tile: value.defaults.selection.tile.name,
          board: value.defaults.selection.board.name,
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
        if (!touched && catalog.defaults.legacyCompatible) {
          request.current = intent;
        } else {
          const rules = matchSettings.safeParse(
            Object.fromEntries(
              numericFields.map((key) => [
                key,
                edits[key].trim() === "" ? NaN : Number(edits[key]),
              ]),
            ),
          );
          const wordList = catalog.wordLists.find(
            (row) => row.identity.name === choices.wordList,
          )?.identity;
          const tile = catalog.tileProfiles.find(
            (row) => row.identity.name === choices.tile,
          )?.identity;
          const board = catalog.boardProfiles.find(
            (row) => row.identity.name === choices.board,
          )?.identity;
          if (!rules.success) {
            throw new CreationError("profiles.invalidRules", false);
          }
          if (!wordList || !tile || !board) {
            throw new CreationError("profiles.unknown", false);
          }
          request.current = {
            ...intent,
            version: 3,
            selection: { wordList, tile, board },
            rules: rules.data,
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
        <LiveBoard
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
          <div className="match-profile-fields">
            <label>
              {t("profiles.wordList")}
              <NativeSelect
                aria-label={t("profiles.wordList")}
                value={choices.wordList}
                onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                  change();
                  setChoices({ ...choices, wordList: event.currentTarget.value });
                }}
              >
                {catalog.wordLists.map((row) => (
                  <NativeSelectOption key={row.identity.name} value={row.identity.name}>
                    {row.label}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            </label>
            <label>
              {t("profiles.tiles")}
              <NativeSelect
                aria-label={t("profiles.tiles")}
                value={choices.tile}
                onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                  change();
                  setChoices({ ...choices, tile: event.currentTarget.value });
                }}
              >
                {catalog.tileProfiles.map((row) => (
                  <NativeSelectOption key={row.identity.name} value={row.identity.name}>
                    {row.label}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            </label>
            <label>
              {t("profiles.board")}
              <NativeSelect
                aria-label={t("profiles.board")}
                value={choices.board}
                onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                  change();
                  setChoices({ ...choices, board: event.currentTarget.value });
                }}
              >
                {catalog.boardProfiles.map((row) => (
                  <NativeSelectOption key={row.identity.name} value={row.identity.name}>
                    {row.label}
                  </NativeSelectOption>
                ))}
              </NativeSelect>
            </label>
          </div>
          <details>
            <summary>{t("profiles.distribution")}</summary>
            <p>
              {catalog.tileProfiles
                .find((row) => row.identity.name === choices.tile)
                ?.tiles.map(
                  (tile) =>
                    `${tile.face.kind === "letter" ? tile.face.letter : "□"}: ${tile.value} × ${tile.count}`,
                )
                .join(" · ")}
            </p>
          </details>
          <details>
            <summary>{t("profiles.advanced")}</summary>
            <div className="advanced-match-rules">
              {numericFields.map((key) => (
                <label key={key}>
                  {t(`profiles.${key}`)}
                  <input
                    type="number"
                    step="1"
                    value={edits[key]}
                    onChange={(event: ChangeEvent<HTMLInputElement>) => {
                      change();
                      setEdits({ ...edits, [key]: event.currentTarget.value });
                    }}
                  />
                </label>
              ))}
            </div>
          </details>
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
    </main>
  );
}
