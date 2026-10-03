import {
  contract,
  decodeControl,
  decodeWelcome,
  encodeCommand,
  encodeControl,
  ProtocolError,
  SnapshotReassembler,
  type Snapshot,
} from "./wire";

export type Admission = {
  playerId: string;
  connectionEpoch: number;
  currentTick: bigint;
  tickHz: number;
  maxPlayers: number;
  reconnectGraceTicks: bigint;
};
export type ConnectionState =
  | { kind: "idle" | "connecting" | "disconnected" | "closed" }
  | { kind: "connected"; admission: Admission }
  | { kind: "incompatible" | "failed"; message: string };
export type MatchOptions = {
  endpoint: string;
  matchId: string;
  onState: (state: ConnectionState) => void;
  onSnapshot: (snapshot: Snapshot) => void;
  /** Local development certificates only. Production uses normal HTTPS trust. */
  serverCertificateHashes?: WebTransportHash[];
};

export function matchUrl(endpoint: string, matchId: string): URL {
  // Validate before URL parsing, which otherwise normalizes dot segments and backslashes.
  if (
    !/^https:\/\/[^/?#\\]+(?:\/[A-Za-z0-9_-]+)+$/.test(endpoint) ||
    !/^[A-Za-z0-9_-]{1,64}$/.test(matchId)
  ) {
    throw new Error("Invalid server endpoint or match ID");
  }
  const url = new URL(endpoint);
  if (url.username || url.password) {
    throw new Error("Server endpoint must not contain credentials");
  }
  url.pathname += `/matches/${matchId}`;
  return url;
}

function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  signal.throwIfAborted();
  return new Promise<T>((resolve, reject) => {
    const abort = () => reject(signal.reason);
    signal.addEventListener("abort", abort, { once: true });
    promise
      .then(resolve, reject)
      .finally(() => signal.removeEventListener("abort", abort))
      .catch(reject);
  });
}

async function readAll(
  stream: ReadableStream<Uint8Array>,
  maximum: number,
  signal: AbortSignal,
): Promise<Uint8Array> {
  const reader = stream.getReader();
  const cancel = () => {
    reader.cancel().catch(() => {
      /* The owning exchange reports cancellation. */
    });
  };
  signal.addEventListener("abort", cancel, { once: true });
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    signal.throwIfAborted();
    while (true) {
      const result = await reader.read();
      signal.throwIfAborted();
      if (result.done) {
        break;
      }
      length += result.value.length;
      if (length > maximum) {
        throw new ProtocolError("Reliable frame exceeds payload limit");
      }
      chunks.push(result.value);
    }
    const bytes = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.length;
    }
    return bytes;
  } finally {
    signal.removeEventListener("abort", cancel);
    await reader.cancel().catch(() => {
      /* Already failed/closed; caller owns the error. */
    });
    reader.releaseLock();
  }
}

/** Owns one player's in-memory reconnect capability and sequence watermark. No game rules. */
export class BrowserMatch {
  #options: MatchOptions;
  #url: URL;
  #token: Uint8Array | undefined;
  #sequence = 0;
  #transport: WebTransport | undefined;
  #writer: WritableStreamDefaultWriter<Uint8Array> | undefined;
  #admission: Admission | undefined;
  #lifetime: AbortController | undefined;
  #running = false;
  #stopped = false;
  #controls = 0;

  constructor(options: MatchOptions) {
    this.#options = options;
    this.#url = matchUrl(options.endpoint, options.matchId);
  }

  /** Resolves when this connection ends. Calling again explicitly reconnects the same player. */
  async run(): Promise<void> {
    if (this.#running || this.#stopped) {
      throw new Error("Connection already running or closed");
    }
    this.#running = true;
    this.#options.onState({ kind: "connecting" });
    const lifetime = new AbortController();
    this.#lifetime = lifetime;
    let transport: WebTransport | undefined;
    let terminalReported = false;
    try {
      if (typeof WebTransport === "undefined") {
        throw new Error("WebTransport unavailable");
      }
      let path = this.#url.href;
      if (this.#token !== undefined) {
        const token = Array.from(this.#token, (byte) => byte.toString(16).padStart(2, "0")).join(
          "",
        );
        path += `/reconnect/${token}`;
      }
      const options: WebTransportOptions = {};
      if (this.#options.serverCertificateHashes !== undefined) {
        options.serverCertificateHashes = this.#options.serverCertificateHashes;
      }
      transport = new WebTransport(path, options);
      this.#transport = transport;
      // Never expose browser errors: implementations may include the private reconnect URL.
      const close = () => lifetime.abort(new Error("Connection ended"));
      transport.closed.then(close, close);
      const handshake = AbortSignal.any([lifetime.signal, AbortSignal.timeout(10_000)]);
      await abortable(transport.ready, handshake);
      const incoming = transport.incomingUnidirectionalStreams.getReader();
      let welcomeStream: ReadableStream<Uint8Array>;
      try {
        const next = await abortable(incoming.read(), handshake);
        if (next.done) {
          throw new ProtocolError("Missing game-server welcome");
        }
        welcomeStream = next.value;
      } finally {
        incoming.releaseLock();
      }
      const { reconnectToken, ...admission } = decodeWelcome(
        await readAll(welcomeStream, 46, handshake),
      );
      this.#token = reconnectToken;
      this.#admission = admission;
      this.#writer = transport.datagrams.writable.getWriter();
      this.#options.onState({ kind: "connected", admission });
      const reader = transport.datagrams.readable.getReader();
      const reassembler = new SnapshotReassembler();
      try {
        while (!lifetime.signal.aborted) {
          const next = await abortable(reader.read(), lifetime.signal);
          if (next.done) {
            break;
          }
          const snapshot = reassembler.accept(next.value);
          if (snapshot !== undefined) {
            this.#options.onSnapshot(snapshot);
          }
        }
      } finally {
        await reader.cancel().catch(() => {
          /* Connection shutdown owns the read failure. */
        });
        reader.releaseLock();
      }
    } catch (error) {
      terminalReported = true;
      if (!this.#stopped) {
        if (error instanceof ProtocolError) {
          this.#options.onState({ kind: "incompatible", message: error.message });
        } else if (this.#admission !== undefined) {
          this.#options.onState({ kind: "disconnected" });
        } else {
          this.#options.onState({
            kind: "failed",
            message: "Unable to connect. Check the server and browser support.",
          });
        }
      }
    } finally {
      lifetime.abort(new Error("Connection ended"));
      transport?.close();
      if (this.#writer !== undefined) {
        await this.#writer.abort().catch(() => {
          /* Connection shutdown already reports the failure. */
        });
        this.#writer.releaseLock();
      }
      this.#writer = undefined;
      this.#transport = undefined;
      this.#running = false;
      if (this.#stopped) {
        this.#options.onState({ kind: "closed" });
      } else if (!terminalReported) {
        this.#options.onState({ kind: "disconnected" });
      }
    }
  }

  async sendCommand(payload: (sequence: number, playerId: string) => Uint8Array): Promise<number> {
    const writer = this.#writer;
    const admission = this.#admission;
    if (writer === undefined || admission === undefined || this.#lifetime?.signal.aborted) {
      throw new Error("Match is not connected");
    }
    const sequence = this.#sequence + 1;
    const frame = encodeCommand(sequence, payload(sequence, admission.playerId));
    if (frame.length > (this.#transport?.datagrams.maxDatagramSize ?? 0)) {
      throw new ProtocolError("Command exceeds negotiated datagram limit");
    }
    // Reserve before awaiting: concurrent writes and reconnects cannot reuse a sequence.
    this.#sequence = sequence;
    try {
      await writer.write(frame);
    } catch {
      throw new Error("Command delivery failed");
    }
    return sequence;
  }

  async control(payload: Uint8Array): Promise<{ accepted: boolean; payload: Uint8Array }> {
    const frame = encodeControl(payload);
    const transport = this.#transport;
    const lifetime = this.#lifetime;
    if (
      transport === undefined ||
      this.#writer === undefined ||
      lifetime === undefined ||
      lifetime.signal.aborted
    ) {
      throw new Error("Match is not connected");
    }
    if (this.#controls >= 4) {
      throw new Error("Too many control exchanges in flight");
    }
    this.#controls += 1;
    const signal = AbortSignal.any([lifetime.signal, AbortSignal.timeout(5_000)]);
    let stream: WebTransportBidirectionalStream | undefined;
    let writer: WritableStreamDefaultWriter<Uint8Array> | undefined;
    try {
      const opening = transport.createBidirectionalStream().then((opened) => {
        stream = opened;
        if (signal.aborted) {
          opened.readable.cancel().catch(() => {
            /* Expired exchange owns this stream. */
          });
          opened.writable.abort().catch(() => {
            /* Expired exchange owns this stream. */
          });
        }
        return opened;
      });
      const opened = await abortable(opening, signal);
      writer = opened.writable.getWriter();
      await abortable(writer.write(frame), signal);
      await abortable(writer.close(), signal); // FIN is part of the published request framing.
      return decodeControl(
        await readAll(opened.readable, 8 + contract.maxControlPayloadBytes, signal),
      );
    } catch (error) {
      if (error instanceof ProtocolError) {
        throw error;
      }
      throw new Error("Control exchange failed or timed out");
    } finally {
      this.#controls -= 1;
      if (writer !== undefined) {
        await writer.abort().catch(() => {
          /* Closed/failed exchange owns this failure. */
        });
        writer.releaseLock();
      }
      if (stream !== undefined && !stream.readable.locked) {
        await stream.readable.cancel().catch(() => {
          /* Closed/failed exchange owns this failure. */
        });
      }
    }
  }

  disconnect(): void {
    this.#lifetime?.abort(new Error("Disconnected"));
    this.#transport?.close();
  }
  close(): void {
    this.#stopped = true;
    this.#token = undefined;
    this.disconnect();
    if (!this.#running) {
      this.#options.onState({ kind: "closed" });
    }
  }
}
