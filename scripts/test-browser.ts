import { createServer } from "node:net";
const server = createServer();
await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const address = server.address();
if (address === null || typeof address === "string") {
  throw new Error("Missing browser test port");
}
const port = address.port;
await new Promise<void>((resolve, reject) =>
  server.close((error) => {
    if (error) {
      reject(error);
    } else {
      resolve();
    }
  }),
);
const taskEnv: NodeJS.ProcessEnv = { ...process.env, SCRABBLE_BROWSER_PORT: String(port) };
delete taskEnv.NO_COLOR;
const child = Bun.spawn(
  ["bunx", "playwright", "test", "--config", "apps/web/playwright.config.ts"],
  { env: taskEnv, stdout: "inherit", stderr: "inherit" },
);
const stop = () => child.kill("SIGTERM");
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
try {
  process.exitCode = await child.exited;
} finally {
  process.removeListener("SIGINT", stop);
  process.removeListener("SIGTERM", stop);
}
