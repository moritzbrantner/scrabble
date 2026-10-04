import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useState } from "react";
import { JoinError, lookupGame } from "./create-game";
import { LiveBoard } from "./LiveBoard";
import { validInviteId } from "./player-invite";

type State = { kind: "checking" | "ready" } | { kind: "failed"; message: string };
export function JoinRoute({
  matchId,
  endpoint,
  api,
  certificateHash,
}: {
  matchId: string;
  endpoint?: string;
  api?: string;
  certificateHash?: string;
}) {
  const [state, setState] = useState<State>({ kind: "checking" });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    if (!validInviteId(matchId)) {
      setState({
        kind: "failed",
        message: "This invitation has an invalid game identifier. Ask the host for a new link.",
      });
    } else if (endpoint === undefined || api === undefined) {
      setState({
        kind: "failed",
        message: "The game service is not configured. Ask the host for a new invitation.",
      });
    } else {
      setState({ kind: "checking" });
      void lookupGame(api, endpoint, matchId, controller.signal).then(
        () => {
          if (!controller.signal.aborted) {
            setState({ kind: "ready" });
          }
        },
        (error: unknown) => {
          if (!controller.signal.aborted) {
            setState({
              kind: "failed",
              message:
                error instanceof JoinError
                  ? error.message
                  : "Unable to check this game. Check your connection and retry.",
            });
          }
        },
      );
    }
    return () => controller.abort();
  }, [matchId, endpoint, api, attempt]);
  if (state.kind === "ready" && endpoint !== undefined) {
    return (
      <LiveBoard
        endpoint={endpoint}
        matchId={matchId}
        playerView
        {...(certificateHash === undefined ? {} : { certificateHash })}
      />
    );
  }
  return (
    <main className="create-board">
      <h1>Join game</h1>
      {state.kind === "checking" ? (
        <p role="status">Checking game…</p>
      ) : (
        state.kind === "failed" && (
          <>
            <p role="alert">{state.message}</p>
            <Button onClick={() => setAttempt((value) => value + 1)}>Retry join</Button>
          </>
        )
      )}
    </main>
  );
}
