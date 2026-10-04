import { useState } from "react";
import { useCopy } from "./preferences";
import { JoinRoute } from "./JoinRoute";
import { fixtures } from "./fixtures";
import { CreateBoard } from "./CreateBoard";
import { LiveBoard } from "./LiveBoard";
import { SharedBoard } from "./SharedBoard";

export function App() {
  const { t } = useCopy();
  // A preference change must not reinterpret the URL written after game creation and remount its owner.
  const [connection] = useState(() =>
    typeof window === "undefined" ? null : new URLSearchParams(window.location.search),
  );
  const endpoint: string | undefined = import.meta.env.DEV
    ? (connection?.get("server") ?? undefined)
    : import.meta.env.VITE_SCRABBLE_ENDPOINT || undefined;
  const matchId = connection?.get("match");
  const api: string | undefined = import.meta.env.DEV
    ? (connection?.get("api") ?? undefined)
    : import.meta.env.VITE_SCRABBLE_API || undefined;
  let certificateHash: string | undefined;
  if (import.meta.env.DEV) {
    certificateHash = connection?.get("certificate") ?? undefined;
  } else if (import.meta.env.MODE === "test") {
    certificateHash = import.meta.env.VITE_SCRABBLE_TEST_CERTIFICATE_HASH || undefined;
  }
  const localTrust = certificateHash === undefined ? {} : { certificateHash };
  if (connection?.get("view") === "player") {
    const unambiguous =
      connection.getAll("match").length === 1 && connection.getAll("view").length === 1;
    return (
      <JoinRoute
        matchId={unambiguous ? (matchId ?? "") : ""}
        {...(endpoint ? { endpoint } : {})}
        {...(api ? { api } : {})}
        {...localTrust}
      />
    );
  }
  if (endpoint && matchId) {
    return <LiveBoard endpoint={endpoint} matchId={matchId} {...localTrust} />;
  }
  if (endpoint && api) {
    return <CreateBoard endpoint={endpoint} api={api} {...localTrust} />;
  }
  const selected = connection?.get("fixture");
  const fixture = selected === "playing" || selected === "finished" ? selected : "lobby";
  return (
    <>
      <SharedBoard snapshot={fixtures.snapshots[fixture]} rules={fixtures.ruleset} />
      <footer>
        <p className="fixture-note">{t("app.preview")}</p>
      </footer>
    </>
  );
}
