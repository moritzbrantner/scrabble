import { fileURLToPath } from "node:url";
import { z } from "zod";
import { connectionSettings } from "../apps/web/src/connection-settings";

const path = Bun.argv[2] ?? fileURLToPath(new URL("../deploy/client.json", import.meta.url));
const schema = z.strictObject({ endpoint: z.string().max(2048), api: z.string().max(2048) });
let raw: unknown;
try {
  raw = await Bun.file(path).json();
} catch {
  throw new Error("Public Pages configuration must be a readable JSON file");
}
const parsed = schema.safeParse(raw);
if (!parsed.success) {
  throw new Error("Public Pages configuration requires only endpoint and api strings");
}
const settings = connectionSettings("production", parsed.data.endpoint, parsed.data.api);
if (settings.kind === "unavailable" && settings.message === "create.invalidConnection") {
  throw new Error("Public Pages service addresses must be secure and contain no credentials");
}
// Only these public addresses enter the release build. Test pins and other VITE inputs are excluded.
const environment = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.startsWith("VITE_")),
);
const child = Bun.spawn(["bun", "run", "check"], {
  cwd: fileURLToPath(new URL("..", import.meta.url)),
  env: {
    ...environment,
    VITE_SCRABBLE_ENDPOINT: parsed.data.endpoint,
    VITE_SCRABBLE_API: parsed.data.api,
  },
  stdout: "inherit",
  stderr: "inherit",
});
process.exitCode = await child.exited;
