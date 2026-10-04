import { gameApiUrl } from "./create-game";
import { matchUrl } from "./transport/browser-match";

export type ConnectionMode = "development" | "test" | "production";
export type ConnectionSettings =
  | { kind: "configured"; endpoint: string; api: string }
  | { kind: "board"; endpoint: string }
  | { kind: "preview" }
  | { kind: "unavailable"; message: "app.serviceUnavailable" | "create.invalidConnection" };

/** Public deployment addresses only; production never substitutes a fixture game. */
export function connectionSettings(
  mode: ConnectionMode,
  endpoint: string | undefined,
  api: string | undefined,
  route: "game" | "board" = "game",
): ConnectionSettings {
  if (mode === "development" && route === "board") {
    try {
      if (!endpoint) {
        throw new Error("Missing board endpoint");
      }
      matchUrl(endpoint, "validation");
      return { kind: "board", endpoint };
    } catch {
      return { kind: "unavailable", message: "create.invalidConnection" };
    }
  }
  if (!endpoint && !api) {
    return mode === "development"
      ? { kind: "preview" }
      : { kind: "unavailable", message: "app.serviceUnavailable" };
  }
  if (!endpoint || !api) {
    return { kind: "unavailable", message: "create.invalidConnection" };
  }
  try {
    matchUrl(endpoint, "validation");
    const url = gameApiUrl(api);
    if (mode === "production" && url.protocol !== "https:") {
      return { kind: "unavailable", message: "create.invalidConnection" };
    }
    return { kind: "configured", endpoint, api };
  } catch {
    return { kind: "unavailable", message: "create.invalidConnection" };
  }
}
