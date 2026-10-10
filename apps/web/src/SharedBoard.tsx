import { useCopy } from "./preferences";
import { type PublicSnapshot, type Ruleset } from "./public-state";
import { TurnHistory } from "./TurnHistory";

export function SharedBoard({ snapshot, rules }: { snapshot: PublicSnapshot; rules: Ruleset }) {
  const { t, number } = useCopy();
  const cells = new Map(
    snapshot.board.map((tile) => [`${tile.coordinate.row},${tile.coordinate.column}`, tile]),
  );
  const preview =
    snapshot.phase.kind === "playing" &&
    snapshot.preview?.player_id === snapshot.phase.active_player &&
    snapshot.preview.turn === snapshot.phase.turn
      ? snapshot.preview
      : null;
  const tentative = new Map(
    preview?.tiles.map((tile) => [`${tile.coordinate.row},${tile.coordinate.column}`, tile]) ?? [],
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
      status = t("board.waiting");
      break;
    }
    case "playing": {
      status = t("board.turn", {
        name:
          snapshot.players.find((player) => player.id === active)?.display_name ?? t("app.player"),
      });
      break;
    }
    case "finished": {
      const winners = snapshot.players
        .filter(
          (player) =>
            snapshot.phase.kind === "finished" && snapshot.phase.winners.includes(player.id),
        )
        .map((player) => player.display_name);
      status = t(winners.length > 1 ? "board.tie" : "board.winner", {
        names: winners.length > 1 ? winners.join(", ") : (winners[0] ?? t("board.noWinner")),
      });
      break;
    }
  }
  return (
    <main className="shared-board">
      <h1 className="sr-only">Scrabble</h1>
      <table className="board" aria-label={t("board.title")}>
        <caption className="sr-only">
          {t("board.dimensions", { rows: rules.board_size, columns: rules.board_size })}
        </caption>
        <thead>
          <tr>
            <th aria-label={t("board.coordinates")}>
              <span className="sr-only">{t("board.coordinates")}</span>
            </th>
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
              <th scope="row">{number(row + 1)}</th>
              {indices.map((column) => {
                const kind = rules.premiums[row * rules.board_size + column] ?? "normal";
                const committed = cells.get(`${row},${column}`);
                const pending =
                  committed === undefined ? tentative.get(`${row},${column}`) : undefined;
                const tile = committed ?? pending;
                const value = tile?.is_blank ? 0 : values.get(tile?.letter ?? "");
                let description = t(`premium.${kind}`);
                if (tile !== undefined) {
                  const key = pending === undefined ? "board.committed" : "board.preview";
                  description = t(tile.is_blank ? `${key}Blank` : key, {
                    letter: tile.letter,
                    points: value ?? t("score.unknown"),
                  });
                }
                return (
                  <td
                    key={column}
                    aria-label={t("board.square", {
                      row: row + 1,
                      column: column + 1,
                      description,
                    })}
                  >
                    <div className={`board-cell premium-${kind.replaceAll("_", "-")}`}>
                      <span className="premium-mark" aria-hidden="true">
                        {t(`premiumShort.${kind}`)}
                      </span>
                      {tile && (
                        <span
                          className={`letter-tile${tile.is_blank ? " blank-tile" : ""}${pending === undefined ? "" : " tentative-tile"}`}
                          aria-hidden="true"
                        >
                          <span>{tile.letter}</span>
                          <small>{value === undefined ? "?" : number(value)}</small>
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
      <aside className="match-status" aria-label={t("board.status")}>
        <p role="status" className="turn-status">
          {status}
        </p>
        {snapshot.phase.kind === "playing" && (
          <p>{t("board.turnNumber", { turn: BigInt(snapshot.phase.turn) + 1n })}</p>
        )}
        {preview !== null && preview.tiles.length > 0 && <p>{t("board.tentative")}</p>}
        {snapshot.phase.kind === "finished" && <p>{t("board.finished")}</p>}
        {snapshot.players.length > 0 && (
          <ul aria-label={t("board.players")}>
            {snapshot.players.map((player) => (
              <li key={player.id} className={player.id === active ? "active-player" : ""}>
                <span>
                  {player.display_name}
                  {player.connected !== undefined && (
                    <small>
                      {" "}
                      · {t(player.connected ? "board.connected" : "board.disconnected")}
                    </small>
                  )}
                  {player.id === active && <span className="sr-only">{t("board.active")}</span>}
                </span>
                <strong aria-label={t("score.points", { count: player.score })}>
                  {number(player.score)}
                </strong>
              </li>
            ))}
          </ul>
        )}
        {snapshot.phase.kind === "lobby" && <p>{t("board.waitPlayers")}</p>}
        <p className="remaining-tiles">
          {t("board.remaining", { count: snapshot.remaining_tiles })}
        </p>
      </aside>
      <TurnHistory snapshot={snapshot} />
    </main>
  );
}
