import { describe, expect, test } from "bun:test";
import fixtures from "./fixtures/wire.json";
import {
  contract,
  decodeControl,
  decodeCommandRejection,
  decodeSnapshot,
  decodeWelcome,
  encodeCommand,
  encodeControl,
  ProtocolError,
  SnapshotReassembler,
} from "./wire";
import { matchUrl } from "./browser-match";
const text = new TextEncoder();
function bytes(hex: string): Uint8Array {
  return Uint8Array.from(hex.match(/../g) ?? [], (pair) => Number.parseInt(pair, 16));
}
const whole = bytes(fixtures.snapshot);
const fragments = fixtures.fragments.map(bytes);
function at(index: number): Uint8Array {
  const fragment = fragments[index];
  if (fragment === undefined) {
    throw new Error("Missing fixture");
  }
  return fragment.slice();
}
function fragment(tick: bigint, index = 0, count = 2, length = 10): Uint8Array {
  const result = new Uint8Array(14 + length);
  result.set([3, 4]);
  const view = new DataView(result.buffer);
  view.setBigUint64(2, tick);
  view.setUint8(10, index);
  view.setUint8(11, count);
  view.setUint16(12, length);
  return result;
}

describe("published game-server wire contract", () => {
  test("Rust encoders and browser encoders agree; u64s retain precision", () => {
    expect(encodeCommand(4, text.encode('{"version":1}'))).toEqual(bytes(fixtures.command));
    expect(encodeControl(text.encode("ping"))).toEqual(bytes(fixtures.controlRequest));
    expect(decodeControl(bytes(fixtures.controlResponse))).toEqual({
      accepted: true,
      payload: text.encode("pong"),
    });
    expect(decodeControl(bytes(fixtures.controlRejected)).accepted).toBe(false);
    expect(decodeSnapshot(whole).tick).toBe(9007199254740993n);
    const welcome = decodeWelcome(bytes(fixtures.welcome));
    expect(welcome).toEqual({
      playerId: "42",
      tickHz: 20,
      maxPlayers: 4,
      currentTick: 9007199254740993n,
      connectionEpoch: 7,
      reconnectToken: new Uint8Array(16).fill(0xab),
      reconnectGraceTicks: 600n,
    });
  });
  test("private rejection frames preserve sequence and reject malformed boundaries", () => {
    const frame = bytes(fixtures.commandRejection);
    expect(decodeCommandRejection(frame)).toEqual({
      sequence: 4,
      payload: text.encode('{"kind":"invalid_words","words":["TA"]}'),
    });
    for (let length = 0; length < frame.length; length += 1) {
      expect(() => decodeCommandRejection(frame.slice(0, length))).toThrow(ProtocolError);
    }
    expect(() => decodeCommandRejection(bytes(fixtures.command))).toThrow(ProtocolError);
    const zero = frame.slice();
    new DataView(zero.buffer).setUint32(2, 0);
    expect(() => decodeCommandRejection(zero)).toThrow("sequence");
    const oversized = frame.slice();
    new DataView(oversized.buffer).setUint16(6, contract.maxCommandRejectionPayloadBytes + 1);
    expect(() => decodeCommandRejection(oversized)).toThrow("payload limit");
    expect(() => decodeCommandRejection(new Uint8Array([...frame, 0]))).toThrow("length");
  });
  test("bounds, version mismatches, framing and integrity fail clearly", () => {
    for (const sequence of [0, -1, 1.5, 0x100000000]) {
      expect(() => encodeCommand(sequence, text.encode("x"))).toThrow(ProtocolError);
    }
    expect(() => encodeCommand(1, new Uint8Array(contract.maxCommandPayloadBytes + 1))).toThrow(
      "payload limit",
    );
    expect(() => encodeControl(new Uint8Array(contract.maxControlPayloadBytes + 1))).toThrow(
      "payload limit",
    );
    for (const fixture of [whole, bytes(fixtures.welcome), at(0)]) {
      const invalid = fixture.slice();
      invalid[0] = 99;
      const decode =
        fixture[1] === 3
          ? decodeWelcome
          : (value: Uint8Array) => new SnapshotReassembler().accept(value);
      expect(() => decode(invalid)).toThrow("Incompatible");
      expect(() => decode(fixture.slice(0, -1))).toThrow(ProtocolError);
    }
    const corrupt = whole.slice();
    corrupt[corrupt.length - 1] = 0;
    expect(() => decodeSnapshot(corrupt)).toThrow("integrity");
    const control = bytes(fixtures.controlResponse);
    control[4] = 99;
    expect(() => decodeControl(control)).toThrow("Incompatible reliable-control");
    expect(() => decodeControl(new Uint8Array([...bytes(fixtures.controlResponse), 0]))).toThrow(
      "length",
    );
  });
  test("whole and reordered fragmented snapshots converge through one latest-state path", () => {
    const assembler = new SnapshotReassembler();
    expect(assembler.accept(at(2))).toBeUndefined();
    expect(assembler.accept(at(2))).toBeUndefined();
    expect(assembler.accept(at(0))).toBeUndefined();
    expect(assembler.accept(at(1))).toEqual(decodeSnapshot(whole));
    expect(assembler.accept(whole)).toBeUndefined();
    expect(assembler.accept(at(0))).toBeUndefined();
    expect(assembler.pendingSnapshots).toBe(0);
    expect(assembler.bufferedBytes).toBe(0);
    const replacement = new SnapshotReassembler();
    replacement.accept(at(0));
    expect(replacement.accept(whole)).toEqual(decodeSnapshot(whole));
    expect(replacement.pendingSnapshots).toBe(0);
  });
  test("malformed/inconsistent fragments leave unrelated buffers usable", () => {
    const assembler = new SnapshotReassembler();
    assembler.accept(at(0));
    const invalid = at(1);
    invalid[11] = 0;
    expect(() => assembler.accept(invalid)).toThrow(ProtocolError);
    invalid[11] = 4;
    expect(() => assembler.accept(invalid)).toThrow("Inconsistent");
    assembler.accept(at(1));
    expect(assembler.accept(at(2))).toEqual(decodeSnapshot(whole));
    const forged = new SnapshotReassembler();
    const wrongTick = fragments.map((part) => {
      const copy = part.slice();
      new DataView(copy.buffer).setBigUint64(2, 999n);
      return copy;
    });
    for (const part of wrongTick.slice(0, -1)) {
      forged.accept(part);
    }
    const last = wrongTick.at(-1);
    if (last === undefined) {
      throw new Error("Missing fragment");
    }
    expect(() => forged.accept(last)).toThrow("tick mismatch");
    expect(forged.bufferedBytes).toBe(0);
  });
  test("loss, abandoned future ticks and adversarial chunks have bounded storage", () => {
    const assembler = new SnapshotReassembler();
    for (let tick = 1n; tick <= 10n; tick += 1n) {
      assembler.accept(fragment(tick));
    }
    expect(assembler.pendingSnapshots).toBe(contract.maxPendingSnapshots);
    expect(assembler.bufferedBytes).toBe(40);
    assembler.accept(fragment(1n)); // Older than all pending: dropped.
    expect(assembler.bufferedBytes).toBe(40);
    for (let count = 0; count <= contract.maxIdleDatagrams; count += 1) {
      expect(() => assembler.accept(new Uint8Array())).toThrow(ProtocolError);
    }
    expect(assembler.bufferedBytes).toBe(0);
    expect(assembler.pendingSnapshots).toBe(0);
    for (let tick = 1n; tick <= 10n; tick += 1n) {
      assembler.accept(fragment(tick, 0, 2, 65535));
      expect(assembler.bufferedBytes).toBeLessThanOrEqual(contract.maxBufferedSnapshotBytes);
    }
    expect(() => assembler.accept(fragment(10n, 1, 2, 65535))).toThrow("exceeds");
    expect(assembler.bufferedBytes).toBeLessThanOrEqual(contract.maxBufferedSnapshotBytes);
  });
  test("canonical match routes reject browser-normalized and credential-bearing input", () => {
    expect(matchUrl("https://example.com/api/game_v1", "one_1").href).toBe(
      "https://example.com/api/game_v1/matches/one_1",
    );
    for (const endpoint of [
      "http://example.com/game",
      "https://example.com/game/",
      "https://example.com/../game",
      "https://example.com/game?x=1",
      "https://example.com/g%61me",
      "https://u:p@example.com/game",
      "https://example.com/g\\ame",
    ]) {
      expect(() => matchUrl(endpoint, "one")).toThrow();
    }
    for (const match of ["", "../one", "one/two", "ä", "a".repeat(65)]) {
      expect(() => matchUrl("https://example.com/game", match)).toThrow();
    }
  });
});
