import { type Copy } from "./copy";
import { useCopy } from "./preferences";
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState } from "react";
import {
  createGame,
  CreationError,
  newCreateRequest,
  type CreateRequest,
  type JoinInformation,
} from "./create-game";
import { LiveBoard } from "./LiveBoard";

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
  const request = useRef<CreateRequest>(undefined);
  const operation = useRef<AbortController>(undefined);
  useEffect(() => () => operation.current?.abort(), []);
  async function create(fresh: boolean) {
    if (operation.current !== undefined && !operation.current.signal.aborted) {
      return;
    }
    const controller = new AbortController();
    operation.current = controller;
    setState({ kind: "creating" });
    try {
      if (fresh || request.current === undefined) {
        request.current = newCreateRequest();
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
      <p>{t("create.intro")}</p>
      {state.kind === "creating" && <p role="status">{t("create.creating")}</p>}
      {state.kind === "failed" && <p role="alert">{t(state.message)}</p>}
      <Button
        disabled={state.kind === "creating"}
        onClick={() => void create(state.kind !== "failed" || !state.retryable)}
      >
        {state.kind === "failed" && state.retryable ? t("create.retry") : t("create.action")}
      </Button>
    </main>
  );
}
