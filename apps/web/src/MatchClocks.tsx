import { useEffect, useState } from "react";

import { useCopy } from "./preferences";
import { type PublicSnapshot } from "./public-state";

export type ClockObservation = { serverMs: number; receivedAt: number };
/** Only a visual estimate. The server alone applies expiration and advances turns. */
export function MatchClocks({
  snapshot,
  observation,
}: {
  snapshot: PublicSnapshot;
  observation?: ClockObservation;
}) {
  const { t, number } = useCopy();
  const [now, setNow] = useState(() => performance.now());
  const clock = snapshot.clock;
  const control = snapshot.configuration.time_control;
  const running = snapshot.phase.kind === "playing" && clock !== undefined;
  useEffect(() => {
    if (!running) {
      return;
    }
    const timer = setInterval(() => setNow(performance.now()), 100);
    return () => clearInterval(timer);
  }, [running]);
  if (!clock || !control) {
    return null;
  }
  const active = snapshot.phase.kind === "playing" ? snapshot.phase.active_player : undefined;
  const serverNow =
    observation === undefined
      ? undefined
      : observation.serverMs + Math.max(0, now - observation.receivedAt);
  return (
    <section className="match-clocks" aria-label={t("clock.title")}>
      <h2>{t("clock.title")}</h2>
      <p>
        {t(control.clock.kind === "cumulative" ? "clock.cumulative" : "clock.perTurn")} ·{" "}
        {t(control.on_expiry === "pass" ? "clock.pass" : "clock.forfeit")}
      </p>
      <p className="setting-note">
        {control.clock.kind === "cumulative"
          ? t("clock.cumulativeSummary", {
              initial: control.clock.initial_ms / 1000,
              increment: (control.clock.increment_ms ?? 0) / 1000,
            })
          : t("clock.perTurnSummary", { seconds: control.clock.allowance_ms / 1000 })}
      </p>
      <ul>
        {snapshot.players.map((player) => {
          const bank =
            clock.players.find((entry) => entry.player_id === player.id)?.remaining_ms ?? 0;
          const remaining =
            active === player.id && serverNow !== undefined && clock.expires_at_ms !== null
              ? Math.max(0, clock.expires_at_ms - serverNow)
              : bank;
          const tenths = Math.ceil(remaining / 100);
          const seconds = Math.floor(tenths / 10) % 60;
          const text = `${number(Math.floor(tenths / 600))}:${String(seconds).padStart(2, "0")}.${tenths % 10}`;
          return (
            <li key={player.id} className={active === player.id ? "active-player" : ""}>
              <span>
                {player.display_name}
                {player.forfeited && <small> · {t("clock.forfeited")}</small>}
              </span>
              <span
                role="timer"
                aria-label={t("clock.remaining", { name: player.display_name })}
                aria-live="off"
              >
                {text}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
