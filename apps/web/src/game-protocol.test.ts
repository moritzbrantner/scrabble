import { expect, test } from "bun:test";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { ConnectionStatus } from "./ConnectionStatus";
import { fixtures } from "./fixtures";
import {
  commandEnvelope,
  decodePlayerSnapshot,
  decodeWordRejection,
  encodeGameCommand,
  type WordRejection,
} from "./game-protocol";
import { type ConnectionState } from "./transport/browser-match";
import wireFixtures from "./transport/fixtures/wire.json";

const encoder = new TextEncoder();
function snapshot(value: unknown) {
  return encoder.encode(JSON.stringify(value));
}
const player = {
  public: fixtures.snapshots.playing,
  own_rack: { player_id: "1", tiles: [{ id: "9", face: { kind: "blank" } }] },
};
test("game payload version and ownership are validated independently of transport", () => {
  expect(decodePlayerSnapshot(snapshot(player), "1").own_rack.tiles[0]?.id).toBe("9");
  expect(() => decodePlayerSnapshot(snapshot(player), "2")).toThrow("different player");
  expect(() =>
    decodePlayerSnapshot(snapshot({ ...player, public: { ...player.public, version: 2 } }), "1"),
  ).toThrow("incompatible Scrabble snapshot");
  expect(() => decodePlayerSnapshot(snapshot({ ...player, bag: [] }), "1")).toThrow("incompatible");
  expect(() => decodePlayerSnapshot(new Uint8Array([0xff]), "1")).toThrow("encoding");
  expect(() => decodePlayerSnapshot(encoder.encode("{"), "1")).toThrow("encoding");
  const command = {
    version: 1,
    game_id: "1",
    player_id: "1",
    sequence: 1,
    expected_turn: "0",
    command: { kind: "pass" },
  } satisfies Parameters<typeof encodeGameCommand>[0];
  expect(JSON.parse(new TextDecoder().decode(encodeGameCommand(command)))).toEqual(command);
  // A JS/runtime caller can bypass TypeScript; the network boundary still rejects its version.
  // @ts-expect-error Deliberate incompatible protocol fixture.
  expect(() => encodeGameCommand({ ...command, version: 2 })).toThrow(
    "incompatible Scrabble command",
  );
});

test("UI exposes connection transitions and incompatibility without private capabilities", () => {
  const states: ConnectionState[] = [
    { kind: "idle" },
    { kind: "connecting" },
    { kind: "disconnected" },
    { kind: "closed" },
    { kind: "failed", message: "Unable to connect." },
    { kind: "incompatible", message: "Incompatible protocol version" },
    {
      kind: "connected",
      admission: {
        playerId: "1",
        connectionEpoch: 2,
        currentTick: 1n,
        tickHz: 20,
        maxPlayers: 4,
        reconnectGraceTicks: 600n,
      },
    },
  ];
  for (const state of states) {
    const html = renderToStaticMarkup(createElement(ConnectionStatus, { state }));
    expect(html).toContain(
      state.kind === "failed" || state.kind === "incompatible" ? 'role="alert"' : 'role="status"',
    );
    expect(html).not.toContain("reconnect");
  }
});

test("Scrabble payloads match Rust-generated commands and scoped projections", () => {
  const command = commandEnvelope.parse(wireFixtures.gameCommand);
  expect(() =>
    encodeGameCommand({
      ...command,
      command: {
        kind: "preview",
        placements: [{ tile_id: "1", coordinate: { row: 7, column: 7 }, blank_as: "\ud800" }],
      },
    }),
  ).toThrow("Invalid or incompatible");
  expect(JSON.parse(new TextDecoder().decode(encodeGameCommand(command)))).toEqual(
    wireFixtures.gameCommand,
  );
  expect<unknown>(decodePlayerSnapshot(snapshot(wireFixtures.playerSnapshot), "1")).toEqual(
    wireFixtures.playerSnapshot,
  );
});

test("private invalid-word feedback is structured and rejects malformed responses", () => {
  const feedback = {
    version: 1,
    game_id: "1",
    expected_turn: "0",
    error: { kind: "invalid_words", words: ["AA", "TT"] },
  } satisfies WordRejection;
  expect(decodeWordRejection(snapshot(feedback))).toEqual(feedback);
  for (const error of [
    { kind: "invalid_words", words: [] },
    { kind: "invalid_words", words: ["TA", "TA"] },
    { kind: "invalid_words", words: ["bad\nword"] },
    { kind: "invalid_words", words: ["A".repeat(16)] },
    { kind: "identity_mismatch" },
  ]) {
    expect(() => decodeWordRejection(snapshot({ ...feedback, error }))).toThrow("protocol");
  }
  expect(() => decodeWordRejection(new Uint8Array([255]))).toThrow("encoding");
});

test("public history bounds and private exchange-field injection fail at the browser boundary", () => {
  const entry = fixtures.snapshots.finished.history[0];
  expect(entry).toBeDefined();
  if (entry === undefined) {
    throw new Error("Missing authoritative finished turn fixture");
  }
  const projected = (history: unknown) =>
    snapshot({
      ...player,
      public: { ...player.public, history },
    });
  expect(() =>
    decodePlayerSnapshot(
      projected([{ ...entry, action: { kind: "exchange", tile_count: 1, tile_ids: ["99"] } }]),
      "1",
    ),
  ).toThrow("incompatible");
  expect(() =>
    decodePlayerSnapshot(projected(Array.from({ length: 25 }, () => entry)), "1"),
  ).toThrow("incompatible");
  expect(() =>
    decodePlayerSnapshot(
      projected([
        {
          ...entry,
          action: { kind: "commit", words: ["A".repeat(16)], move_score: 1, blank_count: 0 },
        },
      ]),
      "1",
    ),
  ).toThrow("incompatible");
  const widest = { ...entry, scores: [{ player_id: "1", delta: -4294967295, score: -2147483648 }] };
  expect(decodePlayerSnapshot(projected([widest]), "1").public.history[0]?.scores[0]?.delta).toBe(
    -4294967295,
  );
  expect(() =>
    decodePlayerSnapshot(
      projected([
        { ...widest, scores: [{ player_id: "1", delta: -4294967296, score: -2147483648 }] },
      ]),
      "1",
    ),
  ).toThrow("incompatible");
});
