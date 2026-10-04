import { fixtures } from "./fixtures";
import { CreateBoard } from "./CreateBoard";
import { LiveBoard } from "./LiveBoard";
import { SharedBoard } from "./SharedBoard";

export function App() {
  const connection =
    typeof window === "undefined" ? null : new URLSearchParams(window.location.search);
  const endpoint = connection?.get("server");
  const matchId = connection?.get("match");
  const api = connection?.get("api");
  const certificateHash = connection?.get("certificate") ?? undefined;
  const localTrust = certificateHash === undefined ? {} : { certificateHash };
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
