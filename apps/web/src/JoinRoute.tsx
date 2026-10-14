import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useState } from "react";

import { copy, type Copy } from "./copy";
import { JoinError, lookupGame } from "./create-game";
import { playerInviteUrl, validInviteId } from "./player-invite";
import { PlayerPhone } from "./PlayerPhone";
import { useCopy } from "./preferences";

type State = { kind: "checking" | "ready" } | { kind: "failed"; message: Copy };
export function JoinRoute({
  matchId: invitedMatchId,
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
  // A followed rematch replaces the invited match in place, carrying the previous name (#33).
  const [{ matchId, name }, setFollowed] = useState<{ matchId: string; name?: string }>({
    matchId: invitedMatchId,
  });
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
        key={matchId}
        {...(name === undefined ? {} : { initialName: name })}
        onFollowRematch={(join, previousName) => {
          // Rebuild from the invite whitelist so no other query field follows the player.
          const location = playerInviteUrl(
            new URL(window.location.href),
            join.matchId,
            Boolean(import.meta.env.DEV),
            import.meta.env.BASE_URL,
          );
          window.history.replaceState(null, "", location);
          setFollowed({ matchId: join.matchId, name: previousName });
        }}
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
