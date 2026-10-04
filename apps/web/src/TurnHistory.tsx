import { type PublicSnapshot } from "./public-state";
import { useCopy } from "./preferences";

export function TurnHistory({ snapshot }: { snapshot: PublicSnapshot }) {
  const { t, number } = useCopy();
  if (snapshot.history.length === 0) {
    return null;
  }
  const name = (id: string) =>
    snapshot.players.find((player) => player.id === id)?.display_name ?? t("app.player");
  return (
    <section aria-label={t("history.title")} className="turn-history">
      <h2>{t("history.title")}</h2>
      {snapshot.history[0]?.turn !== "0" && (
        <p>{t("history.latest", { count: snapshot.history.length })}</p>
      )}
      <ol>
        {snapshot.history.map((turn) => (
          <li key={turn.turn}>
            <strong>
              {t("history.turn", { turn: BigInt(turn.turn) + 1n, name: name(turn.player_id) })}
            </strong>
            {" — "}
            {turn.action.kind === "commit" && (
              <span>
                {t(
                  turn.action.blank_count === 0
                    ? "history.move"
                    : blankHistoryKey(turn.action.blank_count),
                  {
                    words: turn.action.words.join(", "),
                    points: turn.action.move_score,
                    blanks: turn.action.blank_count,
                  },
                )}
              </span>
            )}
            {turn.action.kind === "pass" && <span>{t("history.pass")}</span>}
            {turn.action.kind === "exchange" && (
              <span>
                {t(turn.action.tile_count === 1 ? "history.exchangeOne" : "history.exchangeOther", {
                  count: turn.action.tile_count,
                })}
              </span>
            )}
            <span className="turn-scores">
              {turn.scores
                .map((score) =>
                  t("history.score", {
                    name: name(score.player_id),
                    delta: number(score.delta, "exceptZero"),
                    score: score.score,
                  }),
                )
                .join(" · ")}
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}

function blankHistoryKey(count: number) {
  return count === 1 ? ("history.moveBlank" as const) : ("history.moveBlanks" as const);
}
