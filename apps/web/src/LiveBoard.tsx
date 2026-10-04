import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState } from "react";
import { ConnectionStatus } from "./ConnectionStatus";
import { fixtures } from "./fixtures";
import { decodePlayerSnapshot, encodeGameCommand } from "./game-protocol";
import { type PublicSnapshot } from "./public-state";
import { SharedBoard } from "./SharedBoard";
import { BrowserMatch, type ConnectionState } from "./transport/browser-match";
import { ProtocolError } from "./transport/wire";

/** Development connection surface; endpoint discovery and player shell are later slices. */
export function LiveBoard({
  endpoint,
  matchId,
  certificateHash,
}: {
  endpoint: string;
  matchId: string;
  certificateHash?: string;
}) {
  const [state, setState] = useState<ConnectionState>({ kind: "idle" });
  const [snapshot, setSnapshot] = useState<PublicSnapshot>();
  const client = useRef<BrowserMatch>(undefined);
  const running = useRef<Promise<void>>(undefined);
  const timeout = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [start, setStart] = useState<
    { kind: "idle" | "pending" } | { kind: "failed"; message: string }
  >({ kind: "idle" });
  useEffect(() => {
    let mounted = true;
    let playerId: string | undefined;
    let match: BrowserMatch | undefined;
    try {
      let localTrust: WebTransportHash[] | undefined;
      if (certificateHash !== undefined) {
        if (!/^[0-9a-f]{64}$/.test(certificateHash)) {
          throw new Error("Invalid local certificate hash");
        }
        const bytes = Uint8Array.from({ length: 32 }, (_, index) =>
          Number.parseInt(certificateHash.slice(index * 2, index * 2 + 2), 16),
        );
        localTrust = [{ algorithm: "sha-256", value: bytes.buffer }];
      }
      match = new BrowserMatch({
        ...(localTrust === undefined ? {} : { serverCertificateHashes: localTrust }),
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
          if (current.phase.kind !== "lobby") {
            clearTimeout(timeout.current);
            setStart({ kind: "idle" });
          }
        },
      });
      client.current = match;
      // A cancelled effect must not begin a network admission (including StrictMode replay).
      void Promise.resolve()
        .then(() => {
          if (mounted) {
            running.current = match?.run();
            return running.current;
          }
        })
        .catch(() => {
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
      if (client.current === match) {
        client.current = undefined;
      }
      clearTimeout(timeout.current);
    };
  }, [endpoint, matchId, certificateHash]);
  async function reconnect() {
    const current = client.current;
    if (current === undefined) {
      return;
    }
    setState({ kind: "connecting" });
    await running.current;
    if (client.current === current) {
      running.current = current.run();
      await running.current;
    }
  }
  const host = snapshot?.players[0]?.id;
  const canStart =
    state.kind === "connected" &&
    snapshot?.phase.kind === "lobby" &&
    host === state.admission.playerId &&
    snapshot.players.length >= fixtures.ruleset.minimum_players;
  async function startGame() {
    const current = client.current;
    if (!canStart || snapshot === undefined || current === undefined || start.kind === "pending") {
      return;
    }
    setStart({ kind: "pending" });
    timeout.current = setTimeout(() => {
      setStart({ kind: "failed", message: "Start was not confirmed. You can retry." });
    }, 5000);
    try {
      await current.sendCommand((sequence, playerId) =>
        encodeGameCommand({
          version: 1,
          game_id: snapshot.game_id,
          player_id: playerId,
          sequence,
          expected_turn: "0",
          command: { kind: "start" },
        }),
      );
    } catch {
      clearTimeout(timeout.current);
      setStart({ kind: "failed", message: "Unable to send Start. Reconnect and retry." });
    }
  }
  return (
    <>
      <ConnectionStatus state={state} />
      {(state.kind === "disconnected" || state.kind === "failed") && (
        <Button onClick={() => void reconnect()}>Reconnect</Button>
      )}
      {snapshot?.phase.kind === "lobby" && (
        <section aria-label="Lobby controls">
          <Button disabled={!canStart || start.kind === "pending"} onClick={() => void startGame()}>
            {start.kind === "pending" ? "Starting…" : "Start game"}
          </Button>
          {snapshot.players.length < fixtures.ruleset.minimum_players && (
            <p>At least {fixtures.ruleset.minimum_players} players are needed.</p>
          )}
          {state.kind === "connected" && host !== state.admission.playerId && (
            <p>Waiting for the host to start.</p>
          )}
          {start.kind === "failed" && <p role="alert">{start.message}</p>}
        </section>
      )}
      {snapshot !== undefined && <SharedBoard snapshot={snapshot} rules={fixtures.ruleset} />}
    </>
  );
}
