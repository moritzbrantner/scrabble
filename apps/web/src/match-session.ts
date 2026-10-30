// Connection bookkeeping shared by the shared board and the player phone (#212): local
// certificate trust, the admitted identity and tick rate, and the match rules pinned by the
// first snapshot. Role-specific snapshot handling (board claim, phone name) stays in each
// component; the page lifecycle is owned by `BrowserMatch` itself.
import { decodePlayerSnapshot } from "./game-protocol";
import { matchRules, requireMatchRules } from "./match-rules";
import { type ClockObservation } from "./MatchClocks";
import { type PlayerSnapshot, type Ruleset } from "./public-state";
import { type ConnectionState } from "./transport/browser-match";
import { ProtocolError, type Snapshot } from "./transport/wire";

/** Local development trust for a certificate SHA-256 hex digest; production uses HTTPS trust. */
export function localTrust(certificateHash: string | undefined): WebTransportHash[] | undefined {
  if (certificateHash === undefined) {
    return undefined;
  }
  if (!/^[0-9a-f]{64}$/.test(certificateHash)) {
    throw new Error("Invalid local certificate hash");
  }
  const bytes = Uint8Array.from({ length: 32 }, (_, index) =>
    Number.parseInt(certificateHash.slice(index * 2, index * 2 + 2), 16),
  );
  return [{ algorithm: "sha-256", value: bytes.buffer }];
}

/** One connection's admitted identity and its match rules, pinned by the first snapshot. */
export class MatchSession {
  playerId: string | undefined;
  #tickHz: number | undefined;
  #rules: Ruleset | undefined;
  readonly #onRules: (rules: Ruleset) => void;

  constructor(onRules: (rules: Ruleset) => void) {
    this.#onRules = onRules;
  }

  /** Records admission; returns true when a (re)connection starts synchronizing, which
   * resets the caller's per-connection flags. */
  observe(next: ConnectionState): boolean {
    if (next.kind === "connected" || next.kind === "synchronizing") {
      this.playerId = next.admission.playerId;
      this.#tickHz = next.admission.tickHz;
    }
    return next.kind === "synchronizing";
  }

  /** This player's projection with the match rules checked, or undefined before admission.
   * With `gameId`, a snapshot of another game is rejected before any rule is pinned. */
  decode(frame: Snapshot, gameId?: string): PlayerSnapshot | undefined {
    if (this.playerId === undefined) {
      return undefined;
    }
    const projection = decodePlayerSnapshot(frame.payload, this.playerId);
    if (gameId !== undefined && projection.public.game_id !== gameId) {
      throw new ProtocolError("Snapshot belongs to a different match");
    }
    if (this.#rules === undefined) {
      this.#rules = matchRules(projection.public);
      this.#onRules(this.#rules);
    }
    requireMatchRules(projection.public, this.#rules);
    return projection;
  }

  /** The authoritative clock reading carried by a snapshot of a timed match. */
  clock(frame: Snapshot, projection: PlayerSnapshot): ClockObservation | undefined {
    if (!projection.public.clock || this.#tickHz === undefined) {
      return undefined;
    }
    return { serverMs: (Number(frame.tick) * 1000) / this.#tickHz, receivedAt: performance.now() };
  }
}
