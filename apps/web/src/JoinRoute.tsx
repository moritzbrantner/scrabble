import { copy, type Copy } from "./copy";
import { useCopy } from "./preferences";
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useState } from "react";
import { JoinError, lookupGame } from "./create-game";
import { PlayerPhone } from "./PlayerPhone";
import { validInviteId } from "./player-invite";

type State = { kind: "checking" | "ready" } | { kind: "failed"; message: Copy };
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
  const { t } = useCopy();
  const [state, setState] = useState<State>({ kind: "checking" });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    if (!validInviteId(matchId)) {
      setState({
        kind: "failed",
        message: copy("join.invalidInvite"),
      });
    } else if (endpoint === undefined || api === undefined) {
      setState({
        kind: "failed",
        message: copy("join.unconfigured"),
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
              message: error instanceof JoinError ? error.copy : copy("join.checkFailed"),
            });
          }
        },
      );
    }
    return () => controller.abort();
  }, [matchId, endpoint, api, attempt]);
  if (state.kind === "ready" && endpoint !== undefined && api !== undefined) {
    return (
      <PlayerPhone
        api={api}
        endpoint={endpoint}
        matchId={matchId}
        {...(certificateHash === undefined ? {} : { certificateHash })}
      />
    );
  }
  return (
    <main className="create-board">
      <h1>{t("join.title")}</h1>
      {state.kind === "checking" ? (
        <p role="status">{t("join.checking")}</p>
      ) : (
        state.kind === "failed" && (
          <>
            <p role="alert">{t(state.message)}</p>
            <Button onClick={() => setAttempt((value) => value + 1)}>{t("join.retry")}</Button>
          </>
        )
      )}
    </main>
  );
}
