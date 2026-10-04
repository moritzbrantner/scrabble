import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState } from "react";
import { ConnectionStatus } from "./ConnectionStatus";
import { fixtures } from "./fixtures";
import { matchRules, requireMatchRules } from "./match-rules";
import { decodePlayerSnapshot, encodeGameCommand } from "./game-protocol";
import { type PublicSnapshot, type Ruleset } from "./public-state";
import { PlayerInvite } from "./PlayerInvite";
import { SharedBoard } from "./SharedBoard";
import { BrowserMatch, matchUrl, type ConnectionState } from "./transport/browser-match";
import { readBoardSession, saveBoardSession } from "./transport/resume-capability";
import { ProtocolError } from "./transport/wire";

/** Shared board projection and authenticated Start control. */
export function LiveBoard({
  endpoint,
  matchId,
  certificateHash,
  boardClaim,
}: {
  endpoint: string;
  matchId: string;
  certificateHash?: string;
  boardClaim?: { requestId: string; gameId: string };
}) {
  const separateBoard = matchId.startsWith("b_");
  const [state, setState] = useState<ConnectionState>({ kind: "idle" });
  const [snapshot, setSnapshot] = useState<PublicSnapshot>();
  const [rules, setRules] = useState<Ruleset>(fixtures.ruleset);
  const client = useRef<BrowserMatch>(undefined);
  const running = useRef<Promise<void>>(undefined);
  const claimTimeout = useRef<ReturnType<typeof setTimeout>>(undefined);
  const timeout = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [start, setStart] = useState<
    { kind: "idle" | "pending" } | { kind: "failed"; message: string }
  >({ kind: "idle" });
  useEffect(() => {
    let mounted = true;
    let playerId: string | undefined;
    let match: BrowserMatch | undefined;
    let claimSent = false;
    let selectedRules: Ruleset | undefined;
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
      const resume = separateBoard ? readBoardSession(matchUrl(endpoint, matchId).href) : undefined;
      if (separateBoard && resume === undefined && boardClaim === undefined) {
        throw new Error("Board ownership is unavailable in this tab");
      }
      match = new BrowserMatch({
        ...(resume === undefined ? {} : { resume }),
        ...(separateBoard ? { onResume: saveBoardSession } : {}),
        ...(localTrust === undefined ? {} : { serverCertificateHashes: localTrust }),
        endpoint,
        matchId,
        onState: (next) => {
          if (!mounted) {
            return;
          }
          if (next.kind === "connected") {
            playerId = next.admission.playerId;
            claimSent = false;
          }
          setState(next);
        },
        onSnapshot: (frame) => {
          if (!mounted || playerId === undefined) {
            return;
          }
          const projection = decodePlayerSnapshot(frame.payload, playerId);
          const current = projection.public;
          if (selectedRules === undefined) {
            selectedRules = matchRules(current);
            setRules(selectedRules);
          }
          requireMatchRules(current, selectedRules);
          if (separateBoard) {
            if (current.host === undefined || projection.own_rack.tiles.length !== 0) {
              throw new ProtocolError("Invalid shared board projection");
            }
            if (boardClaim !== undefined && current.game_id !== boardClaim.gameId) {
              throw new ProtocolError("Board claim belongs to a different game");
            }
            if (current.host.id !== null && current.host.id !== playerId) {
              throw new ProtocolError("This connection does not own the board");
            }
            if (current.host.id === null && boardClaim === undefined) {
              throw new ProtocolError("This board session has no confirmed ownership");
            }
            if (current.host.id === null && boardClaim !== undefined && !claimSent) {
              claimSent = true;
              claimTimeout.current = setTimeout(() => {
                if (mounted) {
                  setState({
                    kind: "failed",
                    message: "Board ownership was not confirmed. Reconnect and retry.",
                  });
                }
              }, 5000);
              void match
                ?.sendCommand((sequence, authenticated) =>
                  encodeGameCommand({
                    version: 1,
                    game_id: current.game_id,
                    player_id: authenticated,
                    sequence,
                    expected_turn: "0",
                    command: { kind: "claim_board", request_id: boardClaim.requestId },
                  }),
                )
                .catch(() => {
                  clearTimeout(claimTimeout.current);
                  if (mounted) {
                    setState({
                      kind: "failed",
                      message: "Unable to confirm board ownership. Reconnect and retry.",
                    });
                  }
                });
            }
            if (current.host.id === playerId) {
              clearTimeout(claimTimeout.current);
            }
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
      setState({
        kind: "failed",
        message: separateBoard
          ? "Unable to recover this board session. Use the original board tab or create a new game."
          : "Invalid match connection settings.",
      });
    }
    return () => {
      mounted = false;
      match?.close();
      if (client.current === match) {
        client.current = undefined;
      }
      clearTimeout(timeout.current);
      clearTimeout(claimTimeout.current);
    };
  }, [
    endpoint,
    matchId,
    certificateHash,
    separateBoard,
    boardClaim?.requestId,
    boardClaim?.gameId,
  ]);
  async function reconnect() {
    const current = client.current;
    if (current === undefined) {
      return;
    }
    setState({ kind: "connecting" });
    current.disconnect();
    await running.current;
    if (client.current === current) {
      running.current = current.run();
      await running.current;
    }
  }
  const host = snapshot?.host === undefined ? snapshot?.players[0]?.id : snapshot.host.id;
  const canStart =
    state.kind === "connected" &&
    snapshot?.phase.kind === "lobby" &&
    host === state.admission.playerId &&
    snapshot.players.length >= rules.minimum_players;
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
      {snapshot?.phase.kind === "lobby" && <PlayerInvite key={matchId} matchId={matchId} />}
      {(state.kind === "disconnected" || state.kind === "failed") && (
        <Button onClick={() => void reconnect()}>Reconnect</Button>
      )}
      {snapshot?.phase.kind === "lobby" && (
        <section aria-label="Lobby controls">
          <Button disabled={!canStart || start.kind === "pending"} onClick={() => void startGame()}>
            {start.kind === "pending" ? "Starting…" : "Start game"}
          </Button>
          {snapshot.players.length < rules.minimum_players && (
            <p>At least {rules.minimum_players} players are needed.</p>
          )}
          {state.kind === "connected" && host !== state.admission.playerId && (
            <p>Waiting for the host to start.</p>
          )}
          {start.kind === "failed" && <p role="alert">{start.message}</p>}
        </section>
      )}
      {snapshot !== undefined && <SharedBoard snapshot={snapshot} rules={rules} />}
    </>
  );
}
