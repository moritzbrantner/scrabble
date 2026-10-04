import fixtures from "./fixtures/wire.json" with { type: "json" };

const contract = fixtures.contract;

export { contract };
export class ProtocolError extends Error {
  override name = "ProtocolError";
}

export type Snapshot = { tick: bigint; stateHash: bigint; payload: Uint8Array };
export type Welcome = {
  playerId: string;
  tickHz: number;
  maxPlayers: number;
  currentTick: bigint;
  connectionEpoch: number;
  reconnectToken: Uint8Array;
  reconnectGraceTicks: bigint;
};
const maxFrameBytes = 20 + contract.maxSnapshotPayloadBytes;

function check(condition: boolean, message: string): asserts condition {
  if (!condition) {
    throw new ProtocolError(message);
  }
}
function view(bytes: Uint8Array) {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
}
function header(bytes: Uint8Array, kind: number, minimum: number) {
  check(bytes.length >= minimum, "Truncated game-server frame");
  check(bytes[0] === contract.gameProtocolVersion, "Incompatible game-server protocol version");
  check(bytes[1] === kind, "Unexpected game-server frame kind");
}

export function encodeCommand(sequence: number, payload: Uint8Array): Uint8Array {
  check(
    Number.isInteger(sequence) && sequence > 0 && sequence <= 0xffffffff,
    "Invalid command sequence",
  );
  check(
    payload.length <= contract.maxCommandPayloadBytes,
    "Command exceeds game-server payload limit",
  );
  const bytes = new Uint8Array(8 + payload.length);
  bytes[0] = contract.gameProtocolVersion;
  bytes[1] = 1;
  view(bytes).setUint32(2, sequence);
  view(bytes).setUint16(6, payload.length);
  bytes.set(payload, 8);
  return bytes;
}

export function decodeWelcome(bytes: Uint8Array): Welcome {
  header(bytes, 3, 46);
  check(bytes.length === 46, "Incorrect welcome length");
  const data = view(bytes);
  return {
    playerId: String(data.getUint32(2)),
    tickHz: data.getUint16(6),
    maxPlayers: data.getUint16(8),
    currentTick: data.getBigUint64(10),
    connectionEpoch: data.getUint32(18),
    reconnectToken: bytes.slice(22, 38),
    reconnectGraceTicks: data.getBigUint64(38),
  };
}

export function decodeSnapshot(bytes: Uint8Array): Snapshot {
  header(bytes, 2, 20);
  const data = view(bytes);
  check(bytes.length === 20 + data.getUint16(18), "Incorrect snapshot length");
  const tick = data.getBigUint64(2);
  const stateHash = data.getBigUint64(10);
  const payload = bytes.slice(20);
  const prefix = new Uint8Array(16);
  view(prefix).setBigUint64(0, tick);
  view(prefix).setBigUint64(8, BigInt(payload.length));
  let hash = 0xcbf29ce484222325n;
  for (const part of [prefix, payload]) {
    for (const byte of part) {
      hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
    }
  }
  check(hash === stateHash, "Snapshot integrity check failed");
  return { tick, stateHash, payload };
}

export function encodeControl(payload: Uint8Array): Uint8Array {
  check(
    payload.length <= contract.maxControlPayloadBytes,
    "Control exceeds game-server payload limit",
  );
  const bytes = new Uint8Array(8 + payload.length);
  bytes.set([71, 83, 67, 84, contract.controlFormatVersion, 1]);
  view(bytes).setUint16(6, payload.length);
  bytes.set(payload, 8);
  return bytes;
}

export function decodeControl(bytes: Uint8Array) {
  check(bytes.length >= 8, "Truncated control frame");
  check(
    bytes.slice(0, 4).every((byte, index) => byte === [71, 83, 67, 84][index]),
    "Invalid control magic",
  );
  check(bytes[4] === contract.controlFormatVersion, "Incompatible reliable-control version");
  check(bytes[5] === 2 || bytes[5] === 3, "Unexpected control response kind");
  const length = view(bytes).getUint16(6);
  check(length <= contract.maxControlPayloadBytes, "Control response exceeds payload limit");
  check(bytes.length === 8 + length, "Incorrect control frame length");
  return { accepted: bytes[5] === 2, payload: bytes.slice(8) };
}

type Pending = {
  chunks: (Uint8Array | undefined)[];
  bytes: number;
  received: number;
  storedAt: number;
};

/** One connection, one bounded latest-state channel. Mirrors the published Rust behavior. */
export class SnapshotReassembler {
  #pending = new Map<bigint, Pending>();
  #bytes = 0;
  #datagrams = 0;
  #newest: bigint | undefined;

  get bufferedBytes() {
    return this.#bytes;
  }
  get pendingSnapshots() {
    return this.#pending.size;
  }

  accept(bytes: Uint8Array): Snapshot | undefined {
    this.#datagrams += 1;
    for (const [tick, pending] of this.#pending) {
      if (this.#datagrams - pending.storedAt > contract.maxIdleDatagrams) {
        this.#remove(tick);
      }
    }
    if (bytes[1] !== 4) {
      return this.#deliver(decodeSnapshot(bytes));
    }
    header(bytes, 4, 14);
    const data = view(bytes);
    const tick = data.getBigUint64(2);
    const index = data.getUint8(10);
    const count = data.getUint8(11);
    const length = data.getUint16(12);
    check(
      count > 0 && count <= contract.maxSnapshotFragments && index < count,
      "Invalid snapshot fragment index/count",
    );
    check(length > 0 && bytes.length === 14 + length, "Incorrect snapshot fragment length");
    if (this.#newest !== undefined && tick <= this.#newest) {
      return undefined;
    }
    let pending = this.#pending.get(tick);
    if (pending !== undefined) {
      check(pending.chunks.length === count, "Inconsistent snapshot fragment count");
      if (pending.chunks[index] !== undefined) {
        return undefined;
      }
      if (pending.bytes + length > maxFrameBytes) {
        this.#remove(tick);
        throw new ProtocolError("Reassembled snapshot exceeds payload limit");
      }
    } else if (this.#pending.size >= contract.maxPendingSnapshots && !this.#evictBefore(tick)) {
      return undefined;
    }
    while (this.#bytes + length > contract.maxBufferedSnapshotBytes) {
      if (!this.#evictBefore(tick)) {
        this.#remove(tick);
        return undefined;
      }
    }
    pending = this.#pending.get(tick);
    if (pending === undefined) {
      pending = { chunks: Array.from({ length: count }), bytes: 0, received: 0, storedAt: 0 };
      this.#pending.set(tick, pending);
    }
    pending.chunks[index] = bytes.slice(14);
    pending.bytes += length;
    pending.received += 1;
    pending.storedAt = this.#datagrams;
    this.#bytes += length;
    if (pending.received !== count) {
      return undefined;
    }
    this.#remove(tick);
    const combined = new Uint8Array(pending.bytes);
    let offset = 0;
    for (const chunk of pending.chunks) {
      if (chunk === undefined) {
        throw new ProtocolError("Missing snapshot fragment");
      }
      combined.set(chunk, offset);
      offset += chunk.length;
    }
    const snapshot = decodeSnapshot(combined);
    check(snapshot.tick === tick, "Snapshot fragment tick mismatch");
    return this.#deliver(snapshot);
  }

  #deliver(snapshot: Snapshot): Snapshot | undefined {
    if (this.#newest !== undefined && snapshot.tick <= this.#newest) {
      return undefined;
    }
    this.#newest = snapshot.tick;
    for (const tick of this.#pending.keys()) {
      if (tick <= snapshot.tick) {
        this.#remove(tick);
      }
    }
    return snapshot;
  }
  #remove(tick: bigint) {
    const pending = this.#pending.get(tick);
    if (pending !== undefined) {
      this.#bytes -= pending.bytes;
      this.#pending.delete(tick);
    }
  }
  #evictBefore(tick: bigint): boolean {
    let oldest: bigint | undefined;
    for (const candidate of this.#pending.keys()) {
      if (oldest === undefined || candidate < oldest) {
        oldest = candidate;
      }
    }
    if (oldest !== undefined && oldest < tick) {
      this.#remove(oldest);
      return true;
    }
    return false;
  }
}
