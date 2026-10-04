import { type PublicSnapshot, type Ruleset } from "./public-state";

import { premiums } from "./board-premiums";

export function SharedBoard({ snapshot, rules }: { snapshot: PublicSnapshot; rules: Ruleset }) {
  const cells = new Map(
    snapshot.board.map((tile) => [`${tile.coordinate.row},${tile.coordinate.column}`, tile]),
  );
  const values = new Map(
    rules.tiles.flatMap((tile) =>
      tile.face.kind === "letter" ? [[tile.face.letter, tile.value] as const] : [],
    ),
  );
  const indices = Array.from({ length: rules.board_size }, (_, index) => index);
  const active = snapshot.phase.kind === "playing" ? snapshot.phase.active_player : null;
  let status: string;
  switch (snapshot.phase.kind) {
    case "lobby": {
      status = "Waiting for game";
      break;
    }
    case "playing": {
      status = `${snapshot.players.find((player) => player.id === active)?.display_name ?? "Player"}'s turn`;
      break;
    }
    case "finished": {
      const winners = snapshot.players
        .filter(
          (player) =>
            snapshot.phase.kind === "finished" && snapshot.phase.winners.includes(player.id),
        )
        .map((player) => player.display_name);
      status =
        winners.length > 1 ? `Tie: ${winners.join(", ")}` : `Winner: ${winners[0] ?? "No winner"}`;
      break;
    }
  }
  return (
    <main className="shared-board">
      <h1 className="sr-only">Scrabble</h1>
      <table className="board" aria-label="Scrabble board">
        <caption className="sr-only">
          {rules.board_size} rows and {rules.board_size} columns
        </caption>
        <thead>
          <tr>
            <th aria-label="Coordinates" />
            {indices.map((column) => (
              <th scope="col" key={column}>
                {String.fromCharCode(65 + column)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {indices.map((row) => (
            <tr key={row}>
              <th scope="row">{row + 1}</th>
              {indices.map((column) => {
                const kind = rules.premiums[row * rules.board_size + column] ?? "normal";
                const tile = cells.get(`${row},${column}`);
                const value = tile?.is_blank ? 0 : values.get(tile?.letter ?? "");
                const description = tile
                  ? `${tile.letter}, ${value ?? "unknown"} points${tile.is_blank ? ", blank" : ""}`
                  : premiums[kind].label;
                return (
                  <td
                    key={column}
                    aria-label={`Row ${row + 1}, column ${column + 1}: ${description}`}
                  >
                    <div className={`board-cell premium-${kind.replaceAll("_", "-")}`}>
                      <span className="premium-mark" aria-hidden="true">
                        {premiums[kind].short}
                      </span>
                      {tile && (
                        <span
                          className={`letter-tile${tile.is_blank ? " blank-tile" : ""}`}
                          aria-hidden="true"
                        >
                          <span>{tile.letter}</span>
                          <small>{value}</small>
                        </span>
                      )}
                    </div>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <aside className="match-status" aria-label="Game status">
        <p role="status" className="turn-status">
          {status}
        </p>
        {snapshot.phase.kind === "playing" && <p>Turn {BigInt(snapshot.phase.turn) + 1n}</p>}
        {snapshot.phase.kind === "finished" && <p>Game finished</p>}
        {snapshot.players.length > 0 && (
          <ul aria-label="Players and scores">
            {snapshot.players.map((player) => (
              <li key={player.id} className={player.id === active ? "active-player" : ""}>
                <span>
                  {player.display_name}
                  {player.connected !== undefined && (
                    <small> · {player.connected ? "Connected" : "Disconnected"}</small>
                  )}
                  {player.id === active && <span className="sr-only"> (active)</span>}
                </span>
                <strong aria-label={`${player.score} points`}>{player.score}</strong>
              </li>
            ))}
          </ul>
        )}
        {snapshot.phase.kind === "lobby" && <p>Waiting for players to join.</p>}
        <p className="remaining-tiles">{snapshot.remaining_tiles} tiles remaining</p>
      </aside>
    </main>
  );
}
