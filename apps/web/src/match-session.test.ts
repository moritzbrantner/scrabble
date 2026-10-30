import { expect, test } from "bun:test";

import { fixtures } from "./fixtures";
import { localTrust, MatchSession } from "./match-session";
import { type Ruleset } from "./public-state";

const encoder = new TextEncoder();
const admission = {
  playerId: "1",
  connectionEpoch: 1,
  currentTick: 0n,
  tickHz: 20,
  maxPlayers: 4,
  reconnectGraceTicks: 600n,
};
const frame = (publicState: unknown, tick = 40n) => ({
  tick,
  stateHash: 0n,
  payload: encoder.encode(
    JSON.stringify({ public: publicState, own_rack: { player_id: "1", tiles: [] } }),
  ),
});

test("local trust decodes one SHA-256 digest and rejects anything else", () => {
  expect(localTrust(undefined)).toBeUndefined();
  const trust = localTrust("ab".repeat(32));
  expect(new Uint8Array(trust![0]!.value as ArrayBuffer)).toEqual(new Uint8Array(32).fill(0xab));
  expect(() => localTrust("AB".repeat(32))).toThrow("certificate hash");
});

test("a session decodes only after admission and pins the first snapshot's rules", () => {
  const pinned: Ruleset[] = [];
  const session = new MatchSession((rules) => pinned.push(rules));
  const playing = fixtures.snapshots.playing;
  expect(session.decode(frame(playing))).toBeUndefined();
  expect(session.observe({ kind: "synchronizing", admission })).toBe(true);
  expect(session.observe({ kind: "connected", admission })).toBe(false);
  expect(session.decode(frame(playing))?.public.game_id).toBe(playing.game_id);
  session.decode(frame(playing));
  expect(pinned).toHaveLength(1);
  expect(() => session.decode(frame(playing), "999")).toThrow("different match");
  const decoded = session.decode(frame(playing))!;
  const clock = session.clock(frame(playing, 40n), decoded);
  expect(clock === undefined ? undefined : clock.serverMs).toBe(
    playing.clock === undefined ? undefined : 2000,
  );
});
