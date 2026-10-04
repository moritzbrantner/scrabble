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
  | { kind: "failed"; message: string; retryable: boolean }
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
          error instanceof CreationError
            ? error
            : new CreationError("Unable to create a game.", false);
        setState({ kind: "failed", message: failure.message, retryable: failure.retryable });
      }
    } finally {
      controller.abort();
    }
  }
  if (state.kind === "created") {
    const invite = new URL(window.location.href);
    invite.searchParams.set("match", state.join.matchId);
    // The URL contains public routing only, never the retry nonce or a reconnect token.
    return (
      <>
        <p className="creation-note">
          <a href={invite.href}>Join this game</a>
        </p>
        <LiveBoard
          endpoint={endpoint}
          matchId={state.join.matchId}
          {...(certificateHash === undefined ? {} : { certificateHash })}
        />
      </>
    );
  }
  return (
    <main className="create-board">
      <h1>Scrabble</h1>
      <p>Create a game to get a join link for your players.</p>
      {state.kind === "creating" && <p role="status">Creating game…</p>}
      {state.kind === "failed" && <p role="alert">{state.message}</p>}
      <Button
        disabled={state.kind === "creating"}
        onClick={() => void create(state.kind !== "failed" || !state.retryable)}
      >
        {state.kind === "failed" && state.retryable ? "Retry creation" : "Create game"}
      </Button>
    </main>
  );
}
