import { type ConnectionState } from "./transport/browser-match";

export function ConnectionStatus({ state }: { state: ConnectionState }) {
  switch (state.kind) {
    case "idle":
      return <p role="status">Not connected.</p>;
    case "connecting":
      return <p role="status">Connecting…</p>;
    case "reconnecting":
      return <p role="status">Reconnecting… Attempt {state.attempt} of 3.</p>;
    case "synchronizing":
      return <p role="status">Restoring game…</p>;
    case "connected":
      return <p role="status">Connected · Player {state.admission.playerId}</p>;
    case "disconnected":
      return <p role="status">Disconnected.</p>;
    case "closed":
      return <p role="status">Connection closed.</p>;
    case "failed":
    case "resume-failed":
      return <p role="alert">{state.message}</p>;
    case "incompatible":
      return <p role="alert">Cannot join: {state.message}</p>;
  }
}
