import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { createElement } from "react";
import { ConnectionStatus } from "./ConnectionStatus";
import { fixtures } from "./fixtures";
import wireFixtures from "./transport/fixtures/wire.json";
import { commandEnvelope, decodePlayerSnapshot, encodeGameCommand } from "./game-protocol";
import { type ConnectionState } from "./transport/browser-match";

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
