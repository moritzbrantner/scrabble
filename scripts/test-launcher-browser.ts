import { mkdtemp, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { z } from "zod";

// Cross-repository acceptance deliberately fails when its real prerequisite is
// absent. Ordinary single-repository tests keep using scripts/test-browser.ts.
const setupRoot = resolve(process.env.SCRABBLE_SETUP_REPOSITORY ?? "../multiplayer-setup-service");
if (!(await Bun.file(join(setupRoot, "Cargo.toml")).exists())) {
  throw new Error("Set SCRABBLE_SETUP_REPOSITORY to the multiplayer-setup-service checkout");
}
const target = resolve(process.env.CARGO_TARGET_DIR ?? "target");
const directory = await mkdtemp(join(tmpdir(), "scrabble-launcher-acceptance-"));
const children: Bun.Subprocess[] = [];
async function run(command: string[], cwd: string, env: NodeJS.ProcessEnv = process.env) {
  const child = Bun.spawn(command, { cwd, env, stdout: "inherit", stderr: "inherit" });
  children.push(child);
  if ((await child.exited) !== 0) {
    throw new Error(`Acceptance prerequisite failed: ${command[0]}`);
  }
}
async function unusedPort() {
  const listener = createServer();
  await new Promise<void>((resolve, reject) => {
    listener.once("error", reject);
    listener.listen(0, "127.0.0.1", resolve);
  });
  const address = listener.address();
  if (address === null || typeof address === "string") {
    throw new Error("Missing ephemeral acceptance port");
  }
  await new Promise<void>((resolve, reject) => {
    listener.close((error) => {
      if (error) {
        reject(error);
      } else {
        resolve();
      }
    });
  });
  return address.port;
}
const stop = () => {
  for (const child of children) {
    if (child.exitCode === null) {
      child.kill("SIGINT");
    }
  }
};
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
try {
  await run(
    ["cargo", "build", "-p", "scrabble-server", "--example", "browser_game_fixture", "--locked"],
    process.cwd(),
  );
  await run(["cargo", "build", "--locked"], setupRoot, {
    ...process.env,
    CARGO_TARGET_DIR: target,
  });
  await run(["bun", "run", "build"], join(setupRoot, "web"));
  const gameDirectory = join(directory, "games");
  const game = Bun.spawn(
    [
      join(target, "debug/examples/browser_game_fixture"),
      gameDirectory,
      "https://moritzbrantner.github.io",
    ],
    { stdout: "inherit", stderr: "inherit" },
  );
  children.push(game);
  const metadata = join(gameDirectory, "connection.json");
  const deadline = Date.now() + 15_000;
  while (!(await Bun.file(metadata).exists())) {
    if (game.exitCode !== null || Date.now() > deadline) {
      throw new Error("Real Scrabble fixture did not start");
    }
    await Bun.sleep(25);
  }
  const fixture = z
    .object({ endpoint: z.string(), certificateHash: z.array(z.number()).length(32) })
    .parse(await Bun.file(metadata).json());
  const setupPort = await unusedPort();
  const setup = Bun.spawn([join(target, "debug/multiplayer-setup-service")], {
    env: {
      ...process.env,
      BIND_ADDR: `127.0.0.1:${setupPort}`,
      ROOM_TTL_SECONDS: "60",
      ALLOWED_ORIGINS: "https://moenarch.com",
      RUST_LOG: "error",
    },
    stdout: "inherit",
    stderr: "inherit",
  });
  children.push(setup);
  const setupApi = `http://127.0.0.1:${setupPort}`;
  let ready = false;
  while (!ready) {
    if (setup.exitCode !== null || Date.now() > deadline) {
      throw new Error("Real setup service did not start");
    }
    ready = await fetch(`${setupApi}/health`)
      .then((response) => response.ok)
      .catch(() => false);
    if (!ready) {
      await Bun.sleep(25);
    }
  }
  const pagesBuild = join(directory, "pages");
  await run(
    [
      "bunx",
      "vite",
      "build",
      "--config",
      "apps/web/vite.config.ts",
      "--mode",
      "test",
      "--outDir",
      pagesBuild,
    ],
    process.cwd(),
    {
      ...process.env,
      VITE_SCRABBLE_ENDPOINT: fixture.endpoint,
      VITE_SCRABBLE_API: "https://moenarch.com/scrabble-api",
      VITE_SCRABBLE_TEST_CERTIFICATE_HASH: fixture.certificateHash
        .map((byte) => byte.toString(16).padStart(2, "0"))
        .join(""),
    },
  );
  await run(
    [
      "bunx",
      "playwright",
      "test",
      "--config",
      "scripts/launcher-acceptance.config.ts",
      ...Bun.argv.slice(2),
    ],
    process.cwd(),
    {
      ...process.env,
      SCRABBLE_GAME_FIXTURE: metadata,
      SCRABBLE_ACCEPTANCE_SETUP_API: setupApi,
      SCRABBLE_ACCEPTANCE_SETUP_WEB: join(setupRoot, "web"),
      SCRABBLE_ACCEPTANCE_PAGES: pagesBuild,
    },
  );
} finally {
  stop();
  await Promise.all(children.map((child) => child.exited));
  await rm(directory, { recursive: true, force: true });
  process.removeListener("SIGINT", stop);
  process.removeListener("SIGTERM", stop);
}
