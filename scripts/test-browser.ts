import { createServer } from "node:net";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { z } from "zod";
const pages = Bun.argv.includes("--pages");
const testArgs = Bun.argv.slice(2).filter((argument) => argument !== "--pages");
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
  [
    "cargo",
    "build",
    "-p",
    "scrabble-server",
    "--example",
    "browser_transport_fixture",
    "--example",
    "browser_game_fixture",
    "--locked",
  ],
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
const gameDirectory = join(fixtureDirectory, "games");
const games = Bun.spawn(
  [
    "target/debug/examples/browser_game_fixture",
    gameDirectory,
    `http://127.0.0.1:${port}`,
    ...(pages ? ["playable"] : []),
  ],
  { stdout: "inherit", stderr: "inherit" },
);
const gameMetadataPath = join(gameDirectory, "connection.json");
const metadataPath = join(fixtureDirectory, "connection.json");
try {
  const deadline = Date.now() + 10_000;
  while (!(await Bun.file(metadataPath).exists()) || !(await Bun.file(gameMetadataPath).exists())) {
    if (fixture.exitCode !== null || games.exitCode !== null || Date.now() > deadline) {
      throw new Error("Transport fixture did not start");
    }
    await Bun.sleep(25);
  }
} catch (error) {
  fixture.kill("SIGINT");
  games.kill("SIGINT");
  await Promise.all([fixture.exited, games.exited]);
  await rm(fixtureDirectory, { recursive: true });
  throw error;
}
const taskEnv: NodeJS.ProcessEnv = {
  ...process.env,
  SCRABBLE_BROWSER_PORT: String(port),
  SCRABBLE_TRANSPORT_FIXTURE: metadataPath,
  SCRABBLE_GAME_FIXTURE: gameMetadataPath,
};
delete taskEnv.NO_COLOR;
let child: Bun.Subprocess | undefined;
async function buildClient(directory: string, mode: string, environment: NodeJS.ProcessEnv) {
  child = Bun.spawn(
    [
      "bunx",
      "vite",
      "build",
      "--config",
      "apps/web/vite.config.ts",
      "--mode",
      mode,
      "--outDir",
      `../../test-results/${directory}`,
    ],
    { env: { ...taskEnv, ...environment }, stdout: "inherit", stderr: "inherit" },
  );
  if ((await child.exited) !== 0) {
    throw new Error("Pages test build failed");
  }
}
const run = async () => {
  if (pages) {
    const connection = z
      .object({
        endpoint: z.string(),
        api: z.string(),
        certificateHash: z.array(z.number()).length(32),
      })
      .parse(await Bun.file(gameMetadataPath).json());
    await buildClient("pages-build", "test", {
      VITE_SCRABBLE_ENDPOINT: connection.endpoint,
      VITE_SCRABBLE_API: connection.api,
      VITE_SCRABBLE_TEST_CERTIFICATE_HASH: connection.certificateHash
        .map((byte) => byte.toString(16).padStart(2, "0"))
        .join(""),
    });
    const production = { VITE_SCRABBLE_TEST_CERTIFICATE_HASH: "private-certificate-canary" };
    await buildClient("pages-unconfigured", "production", {
      ...production,
      VITE_SCRABBLE_ENDPOINT: "",
      VITE_SCRABBLE_API: "",
    });
    await buildClient("pages-invalid", "production", {
      ...production,
      VITE_SCRABBLE_ENDPOINT: "https://game.example/game",
      VITE_SCRABBLE_API: "http://game.example/api",
    });
  }
  child = Bun.spawn(
    [
      "bunx",
      "playwright",
      "test",
      "--config",
      pages ? "apps/web/playwright.pages.config.ts" : "apps/web/playwright.config.ts",
      ...testArgs,
    ],
    { env: taskEnv, stdout: "inherit", stderr: "inherit" },
  );
  process.exitCode = await child.exited;
};
const stop = () => {
  child?.kill("SIGTERM");
  fixture.kill("SIGINT");
  games.kill("SIGINT");
};
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
try {
  await run();
} finally {
  fixture.kill("SIGINT");
  games.kill("SIGINT");
  await Promise.all([fixture.exited, games.exited]);
  await rm(fixtureDirectory, { recursive: true });
  process.removeListener("SIGINT", stop);
  process.removeListener("SIGTERM", stop);
}
