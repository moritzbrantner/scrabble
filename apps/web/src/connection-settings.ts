import { gameApiUrl } from "./create-game";
import { matchUrl } from "./transport/browser-match";

export type ConnectionMode = "development" | "test" | "production";
export type ConnectionSettings =
  | { kind: "configured"; endpoint: string; api: string }
  | { kind: "preview" }
  | { kind: "unavailable"; message: "app.serviceUnavailable" | "create.invalidConnection" };

/** Public deployment addresses only; production never substitutes a fixture game. */
export function connectionSettings(
  mode: ConnectionMode,
  endpoint: string | undefined,
  api: string | undefined,
): ConnectionSettings {
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
