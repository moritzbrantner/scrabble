import { useEffect, useState } from "react";
import { ConnectionStatus } from "./ConnectionStatus";
import { fixtures } from "./fixtures";
import { decodePlayerSnapshot } from "./game-protocol";
import { type PublicSnapshot } from "./public-state";
import { SharedBoard } from "./SharedBoard";
import { BrowserMatch, type ConnectionState } from "./transport/browser-match";
import { ProtocolError } from "./transport/wire";

/** Development connection surface; endpoint discovery and player shell are later slices. */
export function LiveBoard({ endpoint, matchId }: { endpoint: string; matchId: string }) {
  const [state, setState] = useState<ConnectionState>({ kind: "idle" });
  const [snapshot, setSnapshot] = useState<PublicSnapshot>();
  useEffect(() => {
    let mounted = true;
    let playerId: string | undefined;
    let match: BrowserMatch | undefined;
    try {
      match = new BrowserMatch({
        endpoint,
        matchId,
        onState: (next) => {
          if (!mounted) {
            return;
          }
          if (next.kind === "connected") {
            playerId = next.admission.playerId;
          }
          setState(next);
        },
        onSnapshot: (frame) => {
          if (!mounted || playerId === undefined) {
            return;
          }
          const current = decodePlayerSnapshot(frame.payload, playerId).public;
          if (
            current.ruleset.name !== fixtures.ruleset.identity.name ||
            current.ruleset.revision !== fixtures.ruleset.identity.revision ||
            current.dictionary.name !== fixtures.ruleset.dictionary.name ||
            current.dictionary.revision !== fixtures.ruleset.dictionary.revision
          ) {
            throw new ProtocolError("This client does not support the match ruleset");
          }
          setSnapshot(current);
        },
      });
      match.run().catch(() => {
        if (mounted) {
          setState({ kind: "failed", message: "Unable to start the connection." });
        }
      });
    } catch {
      setState({ kind: "failed", message: "Invalid match connection settings." });
    }
    return () => {
      mounted = false;
      match?.close();
    };
  }, [endpoint, matchId]);
  return (
    <>
      <ConnectionStatus state={state} />
      {snapshot !== undefined && <SharedBoard snapshot={snapshot} rules={fixtures.ruleset} />}
    </>
  );
}
