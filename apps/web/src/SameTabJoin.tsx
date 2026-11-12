import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useState } from "react";

import { copy, type Copy } from "./copy";
import { JoinError, lookupGame } from "./create-game";
import { playerInviteUrl } from "./player-invite";
import { useCopy } from "./preferences";

type State =
  | { kind: "checking" }
  | { kind: "ready"; href: string }
  | { kind: "failed"; message: Copy };

/** Public admission hint only: the existing player route owns identity and admission. */
export function SameTabJoin({
  endpoint,
  api,
  matchId,
}: {
  endpoint: string;
  api: string;
  matchId: string;
}) {
  const { t } = useCopy();
  const [state, setState] = useState<State>({ kind: "checking" });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setState({ kind: "checking" });
    void lookupGame(api, endpoint, matchId, controller.signal, "new").then(
      () => {
        if (!controller.signal.aborted) {
          try {
            const href = playerInviteUrl(
              new URL(window.location.href),
              matchId,
              Boolean(import.meta.env.DEV),
              import.meta.env.BASE_URL,
            ).href;
            setState({ kind: "ready", href });
          } catch {
            setState({ kind: "failed", message: copy("invite.failed") });
          }
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
    return () => controller.abort();
  }, [endpoint, api, matchId, attempt]);
  if (state.kind === "ready") {
    return <a href={state.href}>{t("invite.sameTab")}</a>;
  }
  if (state.kind === "checking") {
    return <p role="status">{t("join.checking")}</p>;
  }
  return (
    <div>
      <p role="alert">{t(state.message)}</p>
      <Button onClick={() => setAttempt((value) => value + 1)}>{t("join.retry")}</Button>
    </div>
  );
}
