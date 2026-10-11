/**
 * A moenarch launcher session is public routing context, never Scrabble match authority.
 * The authoritative match is always the separate canonical Scrabble match ID.
 */
export type MoenarchLaunch =
  | { kind: "absent" | "invalid" | "unbound" }
  | { kind: "linked"; sessionId: string };

const sessionIdPattern = /^[0123456789ABCDEFGHJKMNPQRSTVWXYZ]{12}$/;
const matchIdPattern = /^[gb]_[0-9a-f]{16}_[0-9a-f]{32}$/;

/** Reject missing/ambiguous mappings instead of accidentally creating a second match. */
export function moenarchLaunch(params: URLSearchParams): MoenarchLaunch {
  const launch = params.getAll("launch");
  const sessions = params.getAll("session");
  if (launch.length === 0 && sessions.length === 0) {
    return { kind: "absent" };
  }
  if (
    launch.length !== 1 ||
    launch[0] !== "moenarch" ||
    sessions.length !== 1 ||
    !sessionIdPattern.test(sessions[0] ?? "")
  ) {
    return { kind: "invalid" };
  }
  const matches = params.getAll("match");
  if (matches.length === 0) {
    return { kind: "unbound" };
  }
  const views = params.getAll("view");
  if (
    matches.length !== 1 ||
    !matchIdPattern.test(matches[0] ?? "") ||
    views.length > 1 ||
    (views.length === 1 && views[0] !== "player")
  ) {
    return { kind: "invalid" };
  }
  return { kind: "linked", sessionId: sessions[0] ?? "" };
}
