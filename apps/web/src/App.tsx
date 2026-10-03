import { fixtures } from "./fixtures";
import { LiveBoard } from "./LiveBoard";
import { SharedBoard } from "./SharedBoard";

export function App() {
  const connection =
    typeof window === "undefined" ? null : new URLSearchParams(window.location.search);
  const endpoint = connection?.get("server");
  const matchId = connection?.get("match");
  if (import.meta.env.DEV && endpoint && matchId) {
    return <LiveBoard endpoint={endpoint} matchId={matchId} />;
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
