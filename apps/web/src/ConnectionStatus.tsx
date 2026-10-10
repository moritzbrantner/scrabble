import { type Copy } from "./copy";
import { useCopy } from "./preferences";
import { type ConnectionState } from "./transport/browser-match";

export function ConnectionStatus({ state, failure }: { state: ConnectionState; failure?: Copy }) {
  const { t } = useCopy();
  switch (state.kind) {
    case "idle":
      return <p role="status">{t("connection.idle")}</p>;
    case "connecting":
      return <p role="status">{t("connection.connecting")}</p>;
    case "reconnecting":
      return <p role="status">{t("connection.reconnecting", { attempt: state.attempt })}</p>;
    case "synchronizing":
      return <p role="status">{t("connection.restoring")}</p>;
    case "connected":
      return (
        <p role="status">
          {t("connection.connected", { player: String(state.admission.playerId) })}
        </p>
      );
    case "disconnected":
      return <p role="status">{t("connection.disconnected")}</p>;
    case "closed":
      return <p role="status">{t("connection.closed")}</p>;
    case "failed":
      return <p role="alert">{t(failure ?? "connection.failed")}</p>;
    case "resume-failed":
      return <p role="alert">{t("session.resumeStopped")}</p>;
    case "incompatible":
      return <p role="alert">{t("connection.incompatible")}</p>;
  }
}
