import { ruleset, type PublicSnapshot, type Ruleset } from "./public-state";
import { ProtocolError } from "./transport/wire";

/** The server supplies immutable public geometry/scoring data; word-list content stays server-side. */
export function matchRules(snapshot: PublicSnapshot): Ruleset {
  const parsed = ruleset.safeParse(snapshot.configuration);
  if (
    !parsed.success ||
    parsed.data.identity.name !== snapshot.ruleset.name ||
    parsed.data.identity.revision !== snapshot.ruleset.revision ||
    parsed.data.dictionary.name !== snapshot.dictionary.name ||
    parsed.data.dictionary.revision !== snapshot.dictionary.revision
  ) {
    throw new ProtocolError("The match ruleset configuration is invalid");
  }
  return parsed.data;
}

export function requireMatchRules(snapshot: PublicSnapshot, rules: Ruleset): void {
  const current = matchRules(snapshot);
  if (JSON.stringify(current) !== JSON.stringify(rules)) {
    throw new ProtocolError("The match ruleset changed during the connection");
  }
}
