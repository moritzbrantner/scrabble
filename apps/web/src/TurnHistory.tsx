import { type PublicSnapshot } from "./public-state";

function signed(value: number): string {
  return value > 0 ? `+${value}` : String(value);
}

export function TurnHistory({ snapshot }: { snapshot: PublicSnapshot }) {
  if (snapshot.history.length === 0) {
    return null;
  }
  const name = (id: string) =>
    snapshot.players.find((player) => player.id === id)?.display_name ?? "Player";
  return (
    <section aria-label="Turn history" className="turn-history">
      <h2>Turn history</h2>
      {snapshot.history[0]?.turn !== "0" && (
        <p>Showing the latest {snapshot.history.length} turns.</p>
      )}
      <ol>
        {snapshot.history.map((turn) => (
          <li key={turn.turn}>
            <strong>
              Turn {BigInt(turn.turn) + 1n}: {name(turn.player_id)}
            </strong>
            {" — "}
            {turn.action.kind === "commit" && (
              <span>
                {turn.action.words.join(", ")} · {turn.action.move_score} points
                {turn.action.blank_count > 0
                  ? ` · ${turn.action.blank_count} blank${turn.action.blank_count === 1 ? "" : "s"}`
                  : ""}
              </span>
            )}
            {turn.action.kind === "pass" && <span>Passed</span>}
            {turn.action.kind === "exchange" && (
              <span>
                Exchanged {turn.action.tile_count} tile{turn.action.tile_count === 1 ? "" : "s"}
              </span>
            )}
            <span className="turn-scores">
              {turn.scores
                .map((score) => `${name(score.player_id)} ${signed(score.delta)} (${score.score})`)
                .join(" · ")}
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}
