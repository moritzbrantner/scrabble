/**
 * A moenarch launcher session is public routing context, never Scrabble match authority.
 * The authoritative match is always the separate canonical Scrabble match ID.
 */
export type MoenarchLaunch =
  | { kind: "absent" | "invalid" | "unbound" }
  | { kind: "hosting" | "linked"; sessionId: string };

const sessionIdPattern = /^[0123456789ABCDEFGHJKMNPQRSTVWXYZ]{12}$/;
const matchIdPattern = /^[gbp]_[0-9a-f]{16}_[0-9a-f]{32}$/;

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
  const views = params.getAll("view");
  if (views.length > 1 || (views.length === 1 && !["player", "board"].includes(views[0] ?? ""))) {
    return { kind: "invalid" };
  }
  if (matches.length === 0) {
    if (views[0] === "board") {
      return { kind: "hosting", sessionId: sessions[0] ?? "" };
    }
    return { kind: "unbound" };
  }
  if (matches.length !== 1 || !matchIdPattern.test(matches[0] ?? "")) {
    return { kind: "invalid" };
  }
  return { kind: "linked", sessionId: sessions[0] ?? "" };
}

/** Invoke only after authoritative board ownership is confirmed; send public routing only. */
export function reportMoenarchMatch(matchId: string): void {
  const page = new URL(window.location.href);
  const launch = moenarchLaunch(page.searchParams);
  const opener: Window | null = window.opener;
  if (
    launch.kind === "linked" &&
    page.searchParams.get("view") === "board" &&
    page.searchParams.get("match") === matchId &&
    opener !== null &&
    !opener.closed
  ) {
    opener.postMessage(
      { type: "scrabble-match-created", session: launch.sessionId, match: matchId },
      "https://moenarch.com",
    );
  }
}
