import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { z } from "zod";
const pages = Bun.argv.includes("--pages");
// Opt-in review scenarios (#185): `--review` selects apps/web/review, and
// `--interactive` keeps the headed browsers open for manual takeover.
const review = Bun.argv.includes("--review");
const interactive = review && Bun.argv.includes("--interactive");
const testArgs = [
  ...Bun.argv
    .slice(2)
    .filter((argument) => !["--pages", "--review", "--interactive"].includes(argument)),
  ...(interactive ? ["--headed"] : []),
];
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
const taskEnv: NodeJS.ProcessEnv = {
  ...process.env,
  SCRABBLE_BROWSER_PORT: String(port),
  ...(interactive ? { SCRABBLE_REVIEW_INTERACTIVE: "1" } : {}),
};
delete taskEnv.NO_COLOR;
let child: Bun.Subprocess | undefined;
process.once("SIGINT", () => child?.kill("SIGTERM"));
process.once("SIGTERM", () => child?.kill("SIGTERM"));
async function playwright(config: string) {
  child = Bun.spawn(
    ["bunx", "playwright", "test", "--config", `apps/web/${config}.config.ts`, ...testArgs],
    { env: taskEnv, stdout: "inherit", stderr: "inherit" },
  );
  process.exitCode = await child.exited;
}

if (!pages && !review) {
  // Development specs start one private native fixture per Playwright worker (browser/fixtures.ts).
  await playwright("playwright");
  process.exit();
}

// A Pages bundle compiles its endpoint in, and review sessions share one fixture
// across files, so these runs provision a single native fixture for every worker.
const fixtureDirectory = await mkdtemp(join(tmpdir(), "scrabble-browser-"));
const gameDirectory = join(fixtureDirectory, "games");
const games = Bun.spawn(
  [
    join(process.env.CARGO_TARGET_DIR ?? "target", "debug/examples/browser_game_fixture"),
    gameDirectory,
    `http://127.0.0.1:${port}`,
    ...(pages ? ["playable"] : []),
  ],
  { stdout: "inherit", stderr: "inherit" },
);
process.once("SIGINT", () => games.kill("SIGINT"));
process.once("SIGTERM", () => games.kill("SIGINT"));
const gameMetadataPath = join(gameDirectory, "connection.json");
const deadline = Date.now() + 10_000;
// The fixture writes its metadata in place, so wait until the whole document parses.
while (
  !(await Bun.file(gameMetadataPath)
    .json()
    .then(
      () => true,
      () => false,
    ))
) {
  if (games.exitCode !== null || Date.now() > deadline) {
    games.kill("SIGINT");
    await games.exited;
    throw new Error(`Game fixture did not start; evidence retained at ${fixtureDirectory}`);
  }
  await Bun.sleep(25);
}
taskEnv.SCRABBLE_GAME_FIXTURE = gameMetadataPath;
async function buildClient(directory: string, mode: string, environment: NodeJS.ProcessEnv) {
  const task = Bun.spawn(
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
  if ((await task.exited) !== 0) {
    throw new Error(`Pages test build ${directory} failed`);
  }
}
try {
  if (pages) {
    const connection = z
      .object({
        endpoint: z.string(),
        api: z.string(),
        certificateHash: z.array(z.number()).length(32),
      })
      .parse(await Bun.file(gameMetadataPath).json());
    const hosted = z
      .object({ endpoint: z.string(), api: z.string() })
      .parse(await Bun.file("deploy/client.json").json());
    const production = { VITE_SCRABBLE_TEST_CERTIFICATE_HASH: "private-certificate-canary" };
    await Promise.all([
      buildClient("pages-build", "test", {
        VITE_SCRABBLE_ENDPOINT: connection.endpoint,
        VITE_SCRABBLE_API: connection.api,
        VITE_SCRABBLE_TEST_CERTIFICATE_HASH: connection.certificateHash
          .map((byte) => byte.toString(16).padStart(2, "0"))
          .join(""),
      }),
      buildClient("pages-unconfigured", "production", {
        ...production,
        VITE_SCRABBLE_ENDPOINT: "",
        VITE_SCRABBLE_API: "",
      }),
      // The moenarch.com bundle, built from the public client addresses (#206).
      buildClient("pages-moenarch", "production", {
        ...production,
        VITE_SCRABBLE_ENDPOINT: hosted.endpoint,
        VITE_SCRABBLE_API: hosted.api,
      }),
      buildClient("pages-invalid", "production", {
        ...production,
        VITE_SCRABBLE_ENDPOINT: "https://game.example/game",
        VITE_SCRABBLE_API: "http://game.example/api",
      }),
    ]);
  }
  await playwright(review ? "playwright.review" : "playwright.pages");
} finally {
  games.kill("SIGINT");
  await games.exited;
  // Preserve an already nonzero browser result; a failed shutdown also fails the command.
  assert.equal(games.exitCode, 0, "native game fixture must finish graceful recovery persistence");
  if (process.exitCode === 0) {
    await rm(fixtureDirectory, { recursive: true });
  } else {
    console.error(`Private browser fixture failure evidence retained at ${fixtureDirectory}`);
  }
}
