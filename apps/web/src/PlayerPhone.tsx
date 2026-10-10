import { Button, Input } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState, type ChangeEvent } from "react";

import { ConnectionStatus } from "./ConnectionStatus";
import { copy, type Copy } from "./copy";
import { JoinError, lookupGame } from "./create-game";
import { fixtures } from "./fixtures";
import {
  decodePlayerSnapshot,
  decodeWordRejection,
  encodeGameCommand,
  type WordRejection,
} from "./game-protocol";
import { matchRules, requireMatchRules } from "./match-rules";
import { MoveEditor } from "./MoveEditor";
import { PlayerRack } from "./PlayerRack";
import { useCopy } from "./preferences";
import { type PlayerSnapshot, type Ruleset } from "./public-state";
import { BrowserMatch, matchUrl, type ConnectionState } from "./transport/browser-match";
import {
  forgetPlayerSession,
  readPlayerSession,
  savePlayerSession,
  type ResumeCapability,
} from "./transport/resume-capability";
import { ProtocolError } from "./transport/wire";

type Intent =
  | { mode: "new"; name: string }
  | { mode: "resume"; resume: ResumeCapability; name?: string };
type State =
  | { kind: "entry"; message?: Copy }
  | { kind: "joining" }
  | { kind: "joined"; message?: Copy }
  | { kind: "failed"; message: Copy };
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
  const { t, number } = useCopy();
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
        message: copy("phone.savedUnreadable"),
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
    let resumeStopped = false;
    let nameTimeout: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    const desiredName = intent.name;
    const fail = (message: Copy) => {
      if (active) {
        setState({ kind: "failed", message });
      }
    };
    const pagehide = () => client?.disconnect();
    window.addEventListener("pagehide", pagehide);
    const connect = async () => {
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
          autoReconnect: true,
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
            if (next.kind === "connected" || next.kind === "synchronizing") {
              playerId = next.admission.playerId;
            }
            if (next.kind === "synchronizing") {
              nameSent = false;
              clearTimeout(nameTimeout);
            }
            if (next.kind === "resume-failed") {
              resumeStopped = true;
              fail(copy("session.resumeStopped"));
            }
            if (next.kind === "disconnected" || next.kind === "failed") {
              fail(savedFailed ? copy("phone.saveFailed") : copy("phone.ended"));
            }
            if (next.kind === "incompatible") {
              fail(copy("connection.incompatible"));
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
              fail(copy("phone.lateSeat"));
              return;
            }
            if (own === undefined && desiredName === undefined) {
              clearTimeout(nameTimeout);
              setState({ kind: "entry", message: copy("phone.chooseName") });
              return;
            }
            setSnapshot(current);
            if (desiredName !== undefined && own?.display_name !== desiredName) {
              if (current.public.phase.kind !== "lobby") {
                clearTimeout(nameTimeout);
                setState({
                  kind: "joined",
                  message: copy("phone.lateName", { name: own?.display_name ?? String(playerId) }),
                });
                return;
              }
              if (!nameSent && current.public.phase.kind === "lobby") {
                nameSent = true;
                nameTimeout = setTimeout(() => fail(copy("phone.nameUnconfirmed")), 5000);
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
                  .catch(() => fail(copy("phone.nameSendFailed")));
              }
              return;
            }
            clearTimeout(nameTimeout);
            setState({ kind: "joined" });
          },
        });
        activeClient.current = client;
        await client.run();
        if (active && playerId === undefined && !savedFailed && !resumeStopped) {
          // Recheck current authority after a raced admission rejection; never allocate a fallback identity.
          await lookupGame(
            api,
            endpoint,
            matchId,
            controller.signal,
            intent.mode === "new" ? "new" : "available",
          );
          fail(intent.mode === "resume" ? copy("phone.resumeFailed") : copy("phone.connectFailed"));
        }
      } catch (error) {
        if (active) {
          fail(error instanceof JoinError ? error.copy : copy("phone.connectFailed"));
        }
      }
    };
    Promise.resolve()
      .then(connect)
      .catch(() => fail(copy("phone.connectFailed")));
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
        message: copy("phone.invalidName"),
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
        message: copy("phone.clearFailed"),
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
      <h1>{state.kind === "joined" ? (own?.display_name ?? t("app.player")) : t("join.title")}</h1>
      {state.kind === "entry" && state.message !== undefined && (
        <p role="alert">{t(state.message)}</p>
      )}
      {state.kind === "entry" && (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            join();
          }}
        >
          <label htmlFor="player-name">{t("join.name")}</label>
          <Input
            id="player-name"
            value={name}
            onChange={(event: ChangeEvent<HTMLInputElement>) => setName(event.target.value)}
            autoComplete="nickname"
            maxLength={128}
          />
          <Button type="submit">{t("join.title")}</Button>
        </form>
      )}
      {state.kind === "joining" && <p role="status">{t("join.joining")}</p>}
      {state.kind === "joined" && state.message !== undefined && (
        <p role="alert">{t(state.message)}</p>
      )}
      {state.kind === "failed" && (
        <>
          <p role="alert">{t(state.message)}</p>
          <Button onClick={retry}>{t("phone.retry")}</Button>
          <Button onClick={reset}>{t("phone.new")}</Button>
        </>
      )}
      {snapshot !== undefined && (
        <>
          <p className="sr-only" data-testid="player-identity">
            {t("phone.identity", { player: snapshot.own_rack.player_id })}
          </p>
          <ConnectionStatus state={connection} />
          {phase?.kind === "lobby" && <p>{t("phone.waitHost")}</p>}
          {phase?.kind === "playing" && (
            <p role="status" aria-live="polite">
              {phase.active_player === snapshot.own_rack.player_id
                ? t("phone.yourTurn")
                : t("phone.waitPlayer", {
                    name:
                      snapshot.public.players.find((player) => player.id === phase.active_player)
                        ?.display_name ?? t("app.activePlayer"),
                  })}
            </p>
          )}
          {phase?.kind === "finished" && <p>{t("phone.finished")}</p>}
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
              <ol className="phone-scores" aria-label={t("board.players")}>
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
                    <strong aria-label={t("score.points", { count: player.score })}>
                      {number(player.score)}
                    </strong>
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
