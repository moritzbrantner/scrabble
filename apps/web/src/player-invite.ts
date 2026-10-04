import QRCode from "qrcode";
import { matchUrl } from "./transport/browser-match";

export function validInviteId(value: string): boolean {
  return /^g_[0-9a-f]{16}_[0-9a-f]{32}$/.test(value);
}

/** Rebuild public routing; never forward arbitrary query fields or fragments. */
export function playerInviteUrl(
  page: URL,
  matchId: string,
  development: boolean,
  base = "/scrabble/",
): URL {
  if (!validInviteId(matchId) || !/^\/(?:[A-Za-z0-9_-]+\/)*$/.test(base)) {
    throw new Error("Invalid game invitation.");
  }
  const invite = new URL(base, page.origin);
  invite.searchParams.set("view", "player");
  invite.searchParams.set("match", matchId);
  if (development) {
    for (const field of ["server", "api", "certificate"]) {
      const values = page.searchParams.getAll(field);
      if (values.length > 1) {
        throw new Error("Ambiguous game invitation.");
      }
      const value = values[0];
      if (value === undefined) {
        continue;
      }
      if (field === "server") {
        matchUrl(value, matchId);
      }
      if (field === "api") {
        const api = new URL(value);
        if (
          !/^https?:\/\/[^/?#\\]+(?:\/[A-Za-z0-9_-]+)*$/.test(value) ||
          api.username ||
          api.password ||
          (api.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(api.hostname))
        ) {
          throw new Error("Invalid game invitation.");
        }
      }
      if (field === "certificate" && !/^[0-9a-f]{64}$/.test(value)) {
        throw new Error("Invalid game invitation.");
      }
      invite.searchParams.set(field, value);
    }
  }
  if (invite.href.length > 512) {
    throw new Error("The invitation is too long to display.");
  }
  return invite;
}

export function inviteQr(url: string): { path: string; size: number; width: number } {
  const modules = QRCode.create(url, { errorCorrectionLevel: "Q" }).modules;
  const paths: string[] = [];
  for (let row = 0; row < modules.size; row++) {
    for (let column = 0; column < modules.size; column++) {
      if (modules.get(row, column)) {
        paths.push(`M${column + 4} ${row + 4}h1v1h-1z`);
      }
    }
  }
  const size = modules.size + 8;
  return { path: paths.join(""), size, width: Math.max(384, size * 5) };
}
