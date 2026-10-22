import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
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
// Completed matches retire after five minutes. Verify replay journeys in
// bounded fixture lifetimes instead of waiting for unrelated tests.
if (!review && testArgs.length === 0 && process.env.SCRABBLE_BROWSER_SUITE === undefined) {
  for (const suite of ["other", "hex", "custom-layout-replay"]) {
    const task = Bun.spawn([process.execPath, ...Bun.argv.slice(1)], {
      env: { ...process.env, SCRABBLE_BROWSER_SUITE: suite },
      stdout: "inherit",
      stderr: "inherit",
    });
    const stopTask = () => task.kill("SIGTERM");
    process.once("SIGINT", stopTask);
    process.once("SIGTERM", stopTask);
    const status = await task.exited;
    process.removeListener("SIGINT", stopTask);
    process.removeListener("SIGTERM", stopTask);
    if (status !== 0) {
      process.exit(status);
    }
  }
  process.exit(0);
}
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
const fixtureExecutables = join(process.env.CARGO_TARGET_DIR ?? "target", "debug", "examples");
const fixture = Bun.spawn(
  [join(fixtureExecutables, "browser_transport_fixture"), fixtureDirectory],
  { stdout: "inherit", stderr: "inherit" },
);
const gameDirectory = join(fixtureDirectory, "games");
const games = Bun.spawn(
  [
    join(fixtureExecutables, "browser_game_fixture"),
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
  console.error(`Browser fixture startup evidence retained at ${fixtureDirectory}`);
  throw error;
}
const taskEnv: NodeJS.ProcessEnv = {
  ...process.env,
  SCRABBLE_BROWSER_PORT: String(port),
  SCRABBLE_TRANSPORT_FIXTURE: metadataPath,
  SCRABBLE_GAME_FIXTURE: gameMetadataPath,
  ...(interactive ? { SCRABBLE_REVIEW_INTERACTIVE: "1" } : {}),
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
      `apps/web/playwright${review ? ".review" : ""}${pages ? ".pages" : ""}.config.ts`,
      ...testArgs,
    ],
    { env: taskEnv, stdout: "inherit", stderr: "inherit" },
  );
  process.exitCode = await child.exited;
};
// A live host has not yet persisted recovery images. Compare real native replay
// only after its existing graceful shutdown, while retaining the completed match
// identities and browser-observed expected results through private fixture files.
const replayGame = z.strictObject({
  game_id: z.string(),
  phase: z.unknown(),
  history: z.array(z.unknown()),
});
const customReplaySelected =
  process.env.SCRABBLE_BROWSER_SUITE === "custom-layout-replay" ||
  testArgs.some((argument) => argument.includes("custom-board-layout-replay"));
const customReplayGame = replayGame.extend({ players: z.array(z.unknown()) });
const customReplayExpectation = z.strictObject({
  match: z.string(),
  configuration: z.unknown(),
  game: customReplayGame,
});
const replayExpectation = z.strictObject({ match: z.string(), game: replayGame });
const replayReport = z.object({
  records_verified: z.int().positive(),
  checkpoints_verified: z.int().positive(),
  canonical_hash: z.string().min(1),
  game: customReplayGame,
});
async function verifyCompletedCustomReplays() {
  const directory = join(gameDirectory, "custom-layout-replay-expectations");
  if (!customReplaySelected && !existsSync(directory)) {
    return;
  }
  assert.deepEqual(
    (await readdir(directory)).sort(),
    ["custom-hex.json", "custom-square.json"],
    "both completed custom geometries must provide native replay expectations",
  );
  for (const entry of ["custom-hex.json", "custom-square.json"]) {
    const expected = customReplayExpectation.parse(
      JSON.parse(await readFile(join(directory, entry), "utf8")),
    );
    const actual = replayReport
      .extend({
        configuration: z.unknown(),
        game: customReplayGame,
      })
      .parse(
        JSON.parse(
          execFileSync(
            join(fixtureExecutables, "browser_game_fixture"),
            ["--hex-replay", gameDirectory, expected.match],
            { encoding: "utf8", timeout: 10_000, stdio: ["ignore", "pipe", "inherit"] },
          ),
        ),
      );
    assert.deepEqual(
      actual.configuration,
      expected.configuration,
      "native replay retains original custom configuration",
    );
    assert.deepEqual(
      actual.game,
      expected.game,
      "native replay retains the SAME completed custom match results and history",
    );
  }
  console.log(
    "Verified 2 completed custom layout matches through native canonical replay after graceful shutdown.",
  );
}
async function verifyCompletedHexReplays() {
  const directory = join(gameDirectory, "hex-replay-expectations");
  if (!existsSync(directory)) {
    return;
  }
  let verified = 0;
  for (const entry of (await readdir(directory)).sort()) {
    assert.match(entry, /^(?:all_word|main_word_only)-(?:tiles|letters)\.json$/);
    const expected = replayExpectation.parse(
      JSON.parse(await readFile(join(directory, entry), "utf8")),
    );
    const actual = replayReport.parse(
      JSON.parse(
        execFileSync(
          join(fixtureExecutables, "browser_game_fixture"),
          ["--hex-replay", gameDirectory, expected.match],
          { encoding: "utf8", timeout: 10_000, stdio: ["ignore", "pipe", "inherit"] },
        ),
      ),
    );
    assert.deepEqual(
      { game_id: actual.game.game_id, phase: actual.game.phase, history: actual.game.history },
      expected.game,
      "native replay must retain the SAME completed match, phase and full history",
    );
    verified += 1;
  }
  assert.ok(verified > 0, "a completed hex expectation directory must not be empty");
  console.log(
    `Verified ${verified} completed hex match(es) through native canonical replay after graceful shutdown.`,
  );
}
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
  let verified = false;
  fixture.kill("SIGINT");
  games.kill("SIGINT");
  try {
    await Promise.all([fixture.exited, games.exited]);
    assert.equal(
      games.exitCode,
      0,
      "native game fixture must finish graceful recovery persistence",
    );
    await verifyCompletedHexReplays();
    await verifyCompletedCustomReplays();
    verified = process.exitCode === 0;
  } finally {
    // Preserve an already nonzero browser result; replay errors also fail the
    // command, and cleanup runs even when verification or shutdown fails.
    if (verified) {
      await rm(fixtureDirectory, { recursive: true });
    } else {
      console.error(`Private browser fixture failure evidence retained at ${fixtureDirectory}`);
    }
    process.removeListener("SIGINT", stop);
    process.removeListener("SIGTERM", stop);
  }
}
