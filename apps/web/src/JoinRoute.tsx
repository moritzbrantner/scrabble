import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useState } from "react";

import { NewGameLink } from "./NewGameLink";
import { copy, type Copy } from "./copy";
import { JoinError, lookupGame } from "./create-game";
import { playerInviteUrl, validInviteId } from "./player-invite";
import { PlayerPhone } from "./PlayerPhone";
import { matchUrl } from "./transport/browser-match";
import { readBoardClaim, readBoardSession } from "./transport/resume-capability";
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
  // Merely opening a public board URL never grants the return/host capability.
  const [boardHref] = useState(() => {
    if (endpoint === undefined || !validInviteId(invitedMatchId)) {
      return undefined;
    }
    try {
      const route = matchUrl(endpoint, invitedMatchId).href;
      if (readBoardSession(route) === undefined && readBoardClaim(route) === undefined) {
        return undefined;
      }
      const url = playerInviteUrl(
        new URL(window.location.href),
        invitedMatchId,
        Boolean(import.meta.env.DEV),
        import.meta.env.BASE_URL,
      );
      url.searchParams.set("view", "board");
      return url.href;
    } catch {
      // Unreadable private storage must not prevent the independent player join flow.
      return undefined;
    }
  });
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
      <>
        {boardHref !== undefined && matchId === invitedMatchId && (
          <a href={boardHref}>{t("invite.returnBoard")}</a>
        )}
        <PlayerPhone
          key={matchId}
          {...(name === undefined ? {} : { initialName: name })}
          onFollowRematch={(join, previousName) => {
            // Rebuild from the invite whitelist so no other query field follows the player.
            // Ambiguous development fields are dropped from the address rather than blocking the
            // follow; this tab keeps its already validated connection settings.
            const page = new URL(window.location.href);
            const base = import.meta.env.BASE_URL;
            let location: URL;
            try {
              location = playerInviteUrl(page, join.matchId, Boolean(import.meta.env.DEV), base);
            } catch {
              location = playerInviteUrl(page, join.matchId, false, base);
            }
            window.history.replaceState(null, "", location);
            setFollowed({ matchId: join.matchId, name: previousName });
          }}
          api={api}
          endpoint={endpoint}
          matchId={matchId}
          {...(certificateHash === undefined ? {} : { certificateHash })}
        />
      </>
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
            <NewGameLink />
            <Button onClick={() => setAttempt((value) => value + 1)}>{t("join.retry")}</Button>
          </>
        )
      )}
    </main>
  );
}
