import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout } from "node:timers/promises";

import { test as base } from "@playwright/test";
import { z } from "zod";

import { bindGameFixture, fixtureExecutable } from "./game-fixture";

// Specs import Playwright through this module so every test runs on its worker's fixture.
export * from "@playwright/test";

const transportSchema = z.strictObject({
  endpoint: z.string(),
  matchId: z.string(),
  certificateHash: z.array(z.int().min(0).max(255)).length(32),
});
/** Completed matches whose native replay the worker verifies after graceful shutdown. */
type NativeReplay = "none" | "hex" | "custom-layout";
type WorkerFixtures = {
  nativeReplay: NativeReplay;
  scrabbleGame: void;
  transportFixture: z.infer<typeof transportSchema>;
};

async function startNative(executable: string, args: string[], metadata: string) {
  const child = spawn(executable, args, { stdio: ["ignore", "inherit", "inherit"] });
  const exited = new Promise<number | null>((resolve) => child.once("exit", resolve));
  const stop = () => {
    child.kill("SIGINT");
    return exited;
  };
  const deadline = Date.now() + 10_000;
  while (!existsSync(metadata)) {
    if (child.exitCode !== null || Date.now() > deadline) {
      await stop();
      throw new Error(`${executable} did not start; evidence retained at ${dirname(metadata)}`);
    }
    await setTimeout(25);
  }
  return stop;
}

/**
 * Every Playwright worker owns a private native game fixture, so specs never share
 * match capacity or retirement windows. Files that need a native replay comparison
 * opt in with `test.use({ nativeReplay })`, which also gives them a dedicated worker
 * whose fixture lifetime is bounded by that one file.
 */
export const test = base.extend<{}, WorkerFixtures>({
  nativeReplay: ["none", { scope: "worker", option: true }],
  scrabbleGame: [
    async ({ nativeReplay }, use) => {
      if (process.env.SCRABBLE_GAME_FIXTURE !== undefined) {
        // Pages, review and launcher acceptance bundle one shared, externally provisioned fixture.
        await use();
        return;
      }
      const origin = `http://127.0.0.1:${process.env.SCRABBLE_BROWSER_PORT}`;
      const directory = join(await mkdtemp(join(tmpdir(), "scrabble-browser-")), "games");
      const metadata = join(directory, "connection.json");
      const stop = await startNative(fixtureExecutable, [directory, origin], metadata);
      bindGameFixture(metadata);
      try {
        await use();
      } finally {
        assert.equal(
          await stop(),
          0,
          "native game fixture must finish graceful recovery persistence",
        );
      }
      if (nativeReplay === "hex") {
        await verifyCompletedHexReplays(directory);
      } else if (nativeReplay === "custom-layout") {
        await verifyCompletedCustomReplays(directory);
      }
      await rm(dirname(directory), { recursive: true });
    },
    { scope: "worker", auto: true },
  ],
  transportFixture: [
    // oxlint-disable-next-line no-empty-pattern -- Playwright reads fixture dependencies from this pattern.
    async ({}, use) => {
      const directory = await mkdtemp(join(tmpdir(), "scrabble-transport-"));
      const metadata = join(directory, "connection.json");
      const executable = join(dirname(fixtureExecutable), "browser_transport_fixture");
      const stop = await startNative(executable, [directory], metadata);
      try {
        await use(transportSchema.parse(JSON.parse(await readFile(metadata, "utf8"))));
      } finally {
        await stop();
      }
      await rm(directory, { recursive: true });
    },
    { scope: "worker" },
  ],
});

// A live host has not yet persisted recovery images. Compare real native replay only
// after its graceful shutdown, using the completed match identities and browser-observed
// expected results the specs retained in private fixture files.
const replayGame = z.strictObject({
  game_id: z.string(),
  phase: z.unknown(),
  history: z.array(z.unknown()),
});
const customReplayGame = replayGame.extend({ players: z.array(z.unknown()) });
const replayReport = z.object({
  records_verified: z.int().positive(),
  checkpoints_verified: z.int().positive(),
  canonical_hash: z.string().min(1),
  game: customReplayGame,
});
function nativeReplay(directory: string, match: string): unknown {
  return JSON.parse(
    execFileSync(fixtureExecutable, ["--hex-replay", directory, match], {
      encoding: "utf8",
      timeout: 10_000,
      stdio: ["ignore", "pipe", "inherit"],
    }),
  );
}

async function verifyCompletedHexReplays(gameDirectory: string) {
  const directory = join(gameDirectory, "hex-replay-expectations");
  if (!existsSync(directory)) {
    return;
  }
  const expectation = z.strictObject({ match: z.string(), game: replayGame });
  const entries = (await readdir(directory)).sort();
  assert.ok(entries.length > 0, "a completed hex expectation directory must not be empty");
  for (const entry of entries) {
    assert.match(entry, /^(?:all_word|main_word_only)-(?:tiles|letters)\.json$/);
    const expected = expectation.parse(JSON.parse(await readFile(join(directory, entry), "utf8")));
    const actual = replayReport.parse(nativeReplay(gameDirectory, expected.match));
    assert.deepEqual(
      { game_id: actual.game.game_id, phase: actual.game.phase, history: actual.game.history },
      expected.game,
      "native replay must retain the SAME completed match, phase and full history",
    );
  }
  console.log(
    `Verified ${entries.length} completed hex match(es) through native canonical replay after graceful shutdown.`,
  );
}

async function verifyCompletedCustomReplays(gameDirectory: string) {
  const directory = join(gameDirectory, "custom-layout-replay-expectations");
  assert.deepEqual(
    existsSync(directory) ? (await readdir(directory)).sort() : [],
    ["custom-hex.json", "custom-square.json"],
    "both completed custom geometries must provide native replay expectations",
  );
  const expectation = z.strictObject({
    match: z.string(),
    configuration: z.unknown(),
    game: customReplayGame,
  });
  for (const entry of ["custom-hex.json", "custom-square.json"]) {
    const expected = expectation.parse(JSON.parse(await readFile(join(directory, entry), "utf8")));
    const actual = replayReport
      .extend({ configuration: z.unknown() })
      .parse(nativeReplay(gameDirectory, expected.match));
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
