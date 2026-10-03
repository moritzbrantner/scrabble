import { fixtures } from "./fixtures";
import { SharedBoard } from "./SharedBoard";

export function App() {
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
