import { JoinRoute } from "./JoinRoute";
import { fixtures } from "./fixtures";
import { CreateBoard } from "./CreateBoard";
import { LiveBoard } from "./LiveBoard";
import { SharedBoard } from "./SharedBoard";

export function App() {
  const connection =
    typeof window === "undefined" ? null : new URLSearchParams(window.location.search);
  const endpoint: string | undefined = import.meta.env.DEV
    ? (connection?.get("server") ?? undefined)
    : import.meta.env.VITE_SCRABBLE_ENDPOINT || undefined;
  const matchId = connection?.get("match");
  const api: string | undefined = import.meta.env.DEV
    ? (connection?.get("api") ?? undefined)
    : import.meta.env.VITE_SCRABBLE_API || undefined;
  const certificateHash = connection?.get("certificate") ?? undefined;
  const localTrust =
    !import.meta.env.DEV || certificateHash === undefined ? {} : { certificateHash };
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
  if (import.meta.env.DEV && endpoint && matchId) {
    return <LiveBoard endpoint={endpoint} matchId={matchId} {...localTrust} />;
  }
  if (import.meta.env.DEV && endpoint && api) {
    return <CreateBoard endpoint={endpoint} api={api} {...localTrust} />;
  }
  const selected =
    typeof window === "undefined"
      ? null
      : new URLSearchParams(window.location.search).get("fixture");
  const fixture = selected === "playing" || selected === "finished" ? selected : "lobby";
  return (
    <>
      <SharedBoard snapshot={fixtures.snapshots[fixture]} rules={fixtures.ruleset} />
      <p className="fixture-note">Board preview · Multiplayer is not connected yet.</p>
    </>
  );
}
