import { createServer } from "node:net";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
const build = Bun.spawn(
  ["cargo", "build", "-p", "scrabble-server", "--example", "browser_transport_fixture", "--locked"],
  { stdout: "inherit", stderr: "inherit" },
);
if ((await build.exited) !== 0) {
  throw new Error("Browser transport fixture build failed");
}
const fixtureDirectory = await mkdtemp(join(tmpdir(), "scrabble-browser-"));
const fixture = Bun.spawn(["target/debug/examples/browser_transport_fixture", fixtureDirectory], {
  stdout: "inherit",
  stderr: "inherit",
});
const metadataPath = join(fixtureDirectory, "connection.json");
try {
  const deadline = Date.now() + 10_000;
  while (!(await Bun.file(metadataPath).exists())) {
    if (fixture.exitCode !== null || Date.now() > deadline) {
      throw new Error("Transport fixture did not start");
    }
    await Bun.sleep(25);
  }
} catch (error) {
  fixture.kill("SIGINT");
  await fixture.exited;
  await rm(fixtureDirectory, { recursive: true });
  throw error;
}
const taskEnv: NodeJS.ProcessEnv = {
  ...process.env,
  SCRABBLE_BROWSER_PORT: String(port),
  SCRABBLE_TRANSPORT_FIXTURE: metadataPath,
};
delete taskEnv.NO_COLOR;
const child = Bun.spawn(
  ["bunx", "playwright", "test", "--config", "apps/web/playwright.config.ts"],
  { env: taskEnv, stdout: "inherit", stderr: "inherit" },
);
const stop = () => {
  child.kill("SIGTERM");
  fixture.kill("SIGINT");
};
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
try {
  process.exitCode = await child.exited;
} finally {
  fixture.kill("SIGINT");
  await fixture.exited;
  await rm(fixtureDirectory, { recursive: true });
  process.removeListener("SIGINT", stop);
  process.removeListener("SIGTERM", stop);
}
