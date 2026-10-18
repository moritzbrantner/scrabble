import { Button } from "@moritzbrantner/ui/client";
import { useState } from "react";

import { useCopy } from "./preferences";
import {
  disableTurnNotifications,
  enableTurnNotifications,
  turnNotificationsEnabled,
  turnNotificationsSupported,
  type ControlExchange,
  type TurnNotificationResult,
} from "./turn-notifications";

/** Per-device opt-in; the permission prompt only follows this deliberate button press. */
export function TurnNotifications({
  gameId,
  playerId,
  control,
}: {
  gameId: string;
  playerId: string;
  control: ControlExchange | undefined;
}) {
  const { t } = useCopy();
  const supported = turnNotificationsSupported();
  const [enabled, setEnabled] = useState(() => turnNotificationsEnabled(gameId, playerId));
  const [result, setResult] = useState<TurnNotificationResult>();
  const [busy, setBusy] = useState(false);
  let message: Parameters<typeof t>[0] = enabled ? "push.enabled" : "push.disabled";
  if (!supported || result === "unsupported") {
    message = "push.unsupported";
  } else if (result === "denied") {
    message = "push.denied";
  } else if (result === "failed") {
    message = "push.failed";
  }
  return (
    <section className="turn-notifications" aria-label={t("push.title")}>
      <Button
        aria-pressed={enabled}
        disabled={!supported || busy || control === undefined}
        onClick={async () => {
          if (control === undefined) {
            return;
          }
          setBusy(true);
          const next = enabled
            ? await disableTurnNotifications(control, gameId, playerId)
            : await enableTurnNotifications(control, gameId, playerId);
          setResult(next);
          setEnabled(turnNotificationsEnabled(gameId, playerId));
          setBusy(false);
        }}
      >
        {t(enabled ? "push.disable" : "push.enable")}
      </Button>
      <p role="status">{t(message)}</p>
    </section>
  );
}
