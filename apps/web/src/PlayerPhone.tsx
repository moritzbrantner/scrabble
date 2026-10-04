import { Button, Input } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState, type ChangeEvent } from "react";
import { JoinError, lookupGame } from "./create-game";
import { ConnectionStatus } from "./ConnectionStatus";
import {
  decodePlayerSnapshot,
  decodeWordRejection,
  encodeGameCommand,
  type WordRejection,
} from "./game-protocol";
import { fixtures } from "./fixtures";
import { matchRules, requireMatchRules } from "./match-rules";
import { type PlayerSnapshot, type Ruleset } from "./public-state";
import { BrowserMatch, matchUrl, type ConnectionState } from "./transport/browser-match";
import {
  forgetPlayerSession,
  readPlayerSession,
  savePlayerSession,
  type ResumeCapability,
} from "./transport/resume-capability";
import { MoveEditor } from "./MoveEditor";
import { PlayerRack } from "./PlayerRack";
import { ProtocolError } from "./transport/wire";

type Intent =
  | { mode: "new"; name: string }
  | { mode: "resume"; resume: ResumeCapability; name?: string };
type State =
  | { kind: "entry"; message?: string }
  | { kind: "joining" }
  | { kind: "joined"; message?: string }
  | { kind: "failed"; message: string };
function validName(name: string): boolean {
  const characters = Array.from(name.trim());
  return (
    characters.length >= 1 &&
    characters.length <= 32 &&
    characters.every((character) => {
      const scalar = character.codePointAt(0);
      return scalar !== undefined && scalar > 31 && (scalar < 127 || scalar > 159);
    })
  );
}

export function PlayerPhone({
  endpoint,
  api,
  matchId,
  certificateHash,
}: {
  endpoint: string;
  api: string;
  matchId: string;
  certificateHash?: string;
}) {
  const route = matchUrl(endpoint, matchId).href;
  const [name, setName] = useState("");
  const [state, setState] = useState<State>({ kind: "entry" });
  const [intent, setIntent] = useState<Intent>();
  const [connection, setConnection] = useState<ConnectionState>({ kind: "idle" });
  const [snapshot, setSnapshot] = useState<PlayerSnapshot>();
  const [rules, setRules] = useState<Ruleset>(fixtures.ruleset);
  const [wordRejection, setWordRejection] = useState<WordRejection>();
  const submittedMove = useRef<{ gameId: string; turn: string; sequence: number }>(undefined);
  const privateResume = useRef<ResumeCapability>(undefined);
  const activeClient = useRef<BrowserMatch>(undefined);
  useEffect(() => {
    try {
      const saved = readPlayerSession(route);
      if (saved !== undefined) {
        privateResume.current = saved;
        setIntent({ mode: "resume", resume: saved });
      }
    } catch {
      setState({
        kind: "failed",
        message:
          "The saved player session cannot be read. You can explicitly join as a new player.",
      });
    }
  }, [route]);
  useEffect(() => {
    if (intent === undefined) {
      return;
    }
    let active = true;
    let client: BrowserMatch | undefined;
    let playerId: string | undefined;
    let nameSent = false;
    let selectedRules: Ruleset | undefined;
    let savedFailed = false;
    let nameTimeout: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    const desiredName = intent.name;
    const fail = (message: string) => {
      if (active) {
        setState({ kind: "failed", message });
      }
    };
    const pagehide = () => client?.disconnect();
    window.addEventListener("pagehide", pagehide);
    void Promise.resolve().then(async () => {
      if (!active) {
        return;
      }
      setState({ kind: "joining" });
      try {
        const join = await lookupGame(
          api,
          endpoint,
          matchId,
          controller.signal,
          intent.mode === "new" ? "new" : "available",
        );
        if (!active) {
          return;
        }
        let trust: WebTransportHash[] | undefined;
        if (certificateHash !== undefined) {
          if (!/^[0-9a-f]{64}$/.test(certificateHash)) {
            throw new Error("Invalid local trust");
          }
          trust = [
            {
              algorithm: "sha-256",
              value: Uint8Array.from({ length: 32 }, (_, index) =>
                Number.parseInt(certificateHash.slice(index * 2, index * 2 + 2), 16),
              ).buffer,
            },
          ];
        }
        client = new BrowserMatch({
          endpoint,
          matchId,
          ...(trust === undefined ? {} : { serverCertificateHashes: trust }),
          ...(intent.mode === "resume" ? { resume: intent.resume } : {}),
          onResume: (capability) => {
            if (!active) {
              return;
            }
            privateResume.current = capability;
            try {
              savePlayerSession(capability);
            } catch {
              savedFailed = true;
              throw new Error("Unable to persist player session");
            }
          },
          onState: (next) => {
            if (!active) {
              return;
            }
            setConnection(next);
            if (next.kind === "connected") {
              playerId = next.admission.playerId;
            }
            if (next.kind === "disconnected" || next.kind === "failed") {
              fail(
                savedFailed
                  ? "Unable to save this player session. Allow tab storage and reconnect."
                  : "The player connection ended. Reconnect to keep your identity.",
              );
            }
            if (next.kind === "incompatible") {
              fail(next.message);
            }
          },
          onCommandRejected: ({ sequence, payload }) => {
            if (!active) {
              return;
            }
            const feedback = decodeWordRejection(payload);
            if (feedback.game_id !== join.gameId) {
              throw new ProtocolError("Rejection belongs to a different match");
            }
            const submitted = submittedMove.current;
            if (
              submitted?.sequence !== sequence ||
              submitted.turn !== feedback.expected_turn ||
              submitted.gameId !== feedback.game_id
            ) {
              return;
            }
            setWordRejection(feedback);
          },
          onSnapshot: (frame) => {
            if (!active || playerId === undefined) {
              return;
            }
            const current = decodePlayerSnapshot(frame.payload, playerId);
            if (current.public.game_id !== join.gameId) {
              throw new ProtocolError("Snapshot belongs to a different match");
            }
            if (selectedRules === undefined) {
              selectedRules = matchRules(current.public);
              setRules(selectedRules);
            }
            requireMatchRules(current.public, selectedRules);
            const own = current.public.players.find((player) => player.id === playerId);
            if (own === undefined && current.public.host === undefined) {
              throw new ProtocolError("Player is absent from the roster");
            }
            if (own === undefined && current.public.phase.kind !== "lobby") {
              clearTimeout(nameTimeout);
              fail(
                "The game started before your player seat was admitted. Ask the host for another invitation.",
              );
              return;
            }
            if (own === undefined && desiredName === undefined) {
              clearTimeout(nameTimeout);
              setState({ kind: "entry", message: "Choose a name to finish joining." });
              return;
            }
            setSnapshot(current);
            if (desiredName !== undefined && own?.display_name !== desiredName) {
              if (current.public.phase.kind !== "lobby") {
                clearTimeout(nameTimeout);
                setState({
                  kind: "joined",
                  message: `The game started before your name was saved. You are joined as ${own?.display_name ?? "Player"}.`,
                });
                return;
              }
              if (!nameSent && current.public.phase.kind === "lobby") {
                nameSent = true;
                nameTimeout = setTimeout(
                  () =>
                    fail("Your name was not confirmed. Reconnect to retry with the same identity."),
                  5000,
                );
                void client
                  ?.sendCommand((sequence, authenticated) =>
                    encodeGameCommand({
                      version: 1,
                      game_id: current.public.game_id,
                      player_id: authenticated,
                      sequence,
                      expected_turn: "0",
                      command: { kind: "set_name", display_name: desiredName },
                    }),
                  )
                  .catch(() => fail("Unable to send your name. Reconnect to retry."));
              }
              return;
            }
            clearTimeout(nameTimeout);
            setState({ kind: "joined" });
          },
        });
        activeClient.current = client;
        await client.run();
        if (active && playerId === undefined && !savedFailed) {
          // Recheck current authority after a raced admission rejection; never allocate a fallback identity.
          await lookupGame(
            api,
            endpoint,
            matchId,
            controller.signal,
            intent.mode === "new" ? "new" : "available",
          );
          fail(
            intent.mode === "resume"
              ? "This player session could not reconnect. Retry, or explicitly join as a new player."
              : "Unable to establish the player connection. Check your browser and network, then retry.",
          );
        }
      } catch (error) {
        if (active) {
          fail(
            error instanceof JoinError
              ? error.message
              : "Unable to establish the player connection. Check your browser and network, then retry.",
          );
        }
      }
    });
    return () => {
      active = false;
      controller.abort();
      clearTimeout(nameTimeout);
      window.removeEventListener("pagehide", pagehide);
      client?.close();
      if (activeClient.current === client) {
        activeClient.current = undefined;
      }
    };
  }, [intent, endpoint, api, matchId, certificateHash]);
  function join() {
    if (!validName(name)) {
      setState({
        kind: "entry",
        message: "Choose a name of 1 to 32 characters without control characters.",
      });
      return;
    }
    const saved = privateResume.current;
    setIntent(
      saved === undefined
        ? { mode: "new", name: name.trim() }
        : { mode: "resume", resume: saved, name: name.trim() },
    );
  }
  function retry() {
    const saved = privateResume.current;
    if (saved === undefined) {
      join();
      return;
    }
    setIntent({ mode: "resume", resume: saved, ...(validName(name) ? { name: name.trim() } : {}) });
  }
  function reset() {
    try {
      activeClient.current?.close();
      forgetPlayerSession(route);
      privateResume.current = undefined;
      setIntent(undefined);
      setSnapshot(undefined);
      setConnection({ kind: "idle" });
      setState({ kind: "entry" });
    } catch {
      setState({
        kind: "failed",
        message: "Unable to clear the saved session. Allow tab storage and retry.",
      });
    }
  }
  const own = snapshot?.public.players.find((player) => player.id === snapshot.own_rack.player_id);
  const phase = snapshot?.public.phase;
  const canAct =
    state.kind === "joined" &&
    connection.kind === "connected" &&
    phase?.kind === "playing" &&
    phase.active_player === snapshot?.own_rack.player_id;
  return (
    <main className="player-phone">
      <h1>{state.kind === "joined" ? (own?.display_name ?? "Player") : "Join game"}</h1>
      {state.kind === "entry" && state.message !== undefined && <p role="alert">{state.message}</p>}
      {state.kind === "entry" && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            join();
          }}
        >
          <label htmlFor="player-name">Player name</label>
          <Input
            id="player-name"
            value={name}
            onChange={(event: ChangeEvent<HTMLInputElement>) => setName(event.target.value)}
            autoComplete="nickname"
            maxLength={128}
          />
          <Button type="submit">Join game</Button>
        </form>
      )}
      {state.kind === "joining" && <p role="status">Joining game…</p>}
      {state.kind === "joined" && state.message !== undefined && (
        <p role="alert">{state.message}</p>
      )}
      {state.kind === "failed" && (
        <>
          <p role="alert">{state.message}</p>
          <Button onClick={retry}>Retry player connection</Button>
          <Button onClick={reset}>Join as a new player</Button>
        </>
      )}
      {snapshot !== undefined && (
        <>
          <p data-testid="player-identity">Player {snapshot.own_rack.player_id}</p>
          <ConnectionStatus state={connection} />
          {phase?.kind === "lobby" && <p>Waiting for the host to start.</p>}
          {phase?.kind === "playing" && (
            <p role="status" aria-live="polite">
              {phase.active_player === snapshot.own_rack.player_id
                ? "It is your turn."
                : `Waiting for ${snapshot.public.players.find((player) => player.id === phase.active_player)?.display_name ?? "the active player"}.`}
            </p>
          )}
          {phase?.kind === "finished" && <p>Game finished.</p>}
          {own !== undefined && phase?.kind !== "lobby" && (
            <>
              {phase?.kind === "playing" ? (
                <MoveEditor
                  snapshot={snapshot}
                  rules={rules}
                  canAct={canAct}
                  {...(wordRejection === undefined ? {} : { wordRejection })}
                  onPreview={async (placements) => {
                    const client = activeClient.current;
                    if (
                      !canAct ||
                      client === undefined ||
                      snapshot.public.phase.kind !== "playing"
                    ) {
                      return;
                    }
                    const turn = snapshot.public.phase.turn;
                    await client.sendCommand((sequence, authenticated) =>
                      encodeGameCommand({
                        version: 1,
                        game_id: snapshot.public.game_id,
                        player_id: authenticated,
                        sequence,
                        expected_turn: turn,
                        command: { kind: "preview", placements },
                      }),
                    );
                  }}
                  onTurnAction={async (command) => {
                    const client = activeClient.current;
                    if (
                      !canAct ||
                      client === undefined ||
                      snapshot.public.phase.kind !== "playing"
                    ) {
                      throw new Error("Turn action cannot be sent");
                    }
                    const turn = snapshot.public.phase.turn;
                    setWordRejection(undefined);
                    await client.sendCommand((sequence, authenticated) => {
                      submittedMove.current =
                        command.kind === "commit"
                          ? { gameId: snapshot.public.game_id, turn, sequence }
                          : undefined;
                      return encodeGameCommand({
                        version: 1,
                        game_id: snapshot.public.game_id,
                        player_id: authenticated,
                        sequence,
                        expected_turn: turn,
                        command,
                      });
                    });
                  }}
                />
              ) : (
                <PlayerRack rack={snapshot.own_rack} rules={rules} canAct={false} />
              )}
              <ol className="phone-scores" aria-label="Players and scores">
                {snapshot.public.players.map((player) => (
                  <li
                    key={player.id}
                    className={
                      phase?.kind === "playing" && phase.active_player === player.id
                        ? "active-player"
                        : ""
                    }
                  >
                    <span>{player.display_name}</span>
                    <strong aria-label={`${player.score} points`}>{player.score}</strong>
                  </li>
                ))}
              </ol>
            </>
          )}
        </>
      )}
    </main>
  );
}
