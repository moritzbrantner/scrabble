import { fixtures } from "./fixtures";
import { type PublicSnapshot, type Ruleset } from "./public-state";
import { ProtocolError } from "./transport/wire";

/** Dictionary content belongs to the server; this client implements the version-one English geometry. */
export function matchRules(snapshot: PublicSnapshot): Ruleset {
  const base = fixtures.ruleset;
  const authored =
    snapshot.ruleset.revision === base.identity.revision &&
    snapshot.dictionary.name === base.dictionary.name &&
    snapshot.dictionary.revision === base.dictionary.revision;
  const configured =
    /^1\+dictionary-sha256:[0-9a-f]{64}$/.test(snapshot.ruleset.revision) &&
    /^[A-Za-z0-9_.-]{1,64}$/.test(snapshot.dictionary.name) &&
    /^[A-Za-z0-9_.-]{1,64}\+sha256:[0-9a-f]{64}$/.test(snapshot.dictionary.revision);
  if (snapshot.ruleset.name !== base.identity.name || (!authored && !configured)) {
    throw new ProtocolError("This client does not support the match ruleset");
  }
  return { ...base, identity: snapshot.ruleset, dictionary: snapshot.dictionary };
}

export function requireMatchRules(snapshot: PublicSnapshot, rules: Ruleset): void {
  if (
    snapshot.ruleset.name !== rules.identity.name ||
    snapshot.ruleset.revision !== rules.identity.revision ||
    snapshot.dictionary.name !== rules.dictionary.name ||
    snapshot.dictionary.revision !== rules.dictionary.revision
  ) {
    throw new ProtocolError("The match ruleset changed during the connection");
  }
}
