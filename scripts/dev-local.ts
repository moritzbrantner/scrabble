import { mkdir, realpath } from "node:fs/promises";
import { createServer } from "node:net";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { z } from "zod";

const root = fileURLToPath(new URL("../", import.meta.url));
const directory = join(root, ".local", "local-server");
const content = await realpath(Bun.argv[2] ?? join(root, ".local", "deployment", "content"));
const manifest = z
  .object({
    dictionaryName: z.string().regex(/^[A-Za-z0-9_.-]{1,64}$/),
    dictionaryRevision: z.string().regex(/^[A-Za-z0-9_.-]{1,64}$/),
    sha256: z.string().regex(/^[0-9a-f]{64}$/),
  })
  .parse(await Bun.file(join(content, "provenance.json")).json());
const words = Bun.file(join(content, "words.txt"));
if (
  words.size > 64 * 1024 * 1024 ||
  new Bun.CryptoHasher("sha256").update(await words.arrayBuffer()).digest("hex") !== manifest.sha256
) {
  throw new Error("Local dictionary differs from its prepared provenance");
}
const uid = process.getuid?.();
const gid = process.getgid?.();
if (
  uid === undefined ||
  gid === undefined ||
  process.env.DOCKER_HOST ||
  process.env.DOCKER_CONTEXT
) {
  throw new Error("Local serving requires Unix ownership and the default local Docker context");
}
const env: NodeJS.ProcessEnv = Object.fromEntries(
  Object.entries(process.env).filter(
    ([key]) =>
      !key.startsWith("SCRABBLE_") && !key.startsWith("DEPLOY_") && !key.startsWith("VITE_"),
  ),
);
Object.assign(env, {
  SCRABBLE_MODE: "local",
  SCRABBLE_CERT_PEM: join(directory, "tls", "cert.pem"),
  SCRABBLE_KEY_PEM: join(directory, "tls", "key.pem"),
  DEPLOY_TLS_DIRECTORY: join(directory, "tls"),
  DEPLOY_STATE_DIRECTORY: join(directory, "state"),
  DEPLOY_CONTENT_DIRECTORY: content,
  DEPLOY_UID: String(uid),
  DEPLOY_GID: String(gid),
  DEPLOY_DICTIONARY_NAME: manifest.dictionaryName,
  DEPLOY_DICTIONARY_REVISION: manifest.dictionaryRevision,
});
const compose = [
  "docker",
  "compose",
  "--env-file",
  "/dev/null",
  "-p",
  "scrabble-local",
  "-f",
  "compose.yaml",
  "-f",
  "compose.dev.yaml",
];
let child: Bun.Subprocess | undefined;
let interrupted = false;
async function run(command: string[]): Promise<string> {
  if (interrupted) {
    throw new Error("Local startup interrupted");
  }
  const commandChild = Bun.spawn(command, { cwd: root, env, stdout: "pipe", stderr: "inherit" });
  child = commandChild;
  const [output, code] = await Promise.all([
    new Response(commandChild.stdout).text(),
    commandChild.exited,
  ]);
  child = undefined;
  if (code !== 0 || interrupted) {
    throw new Error(`Local serving command failed: ${command[0]}`);
  }
  return output;
}
const stop = () => {
  interrupted = true;
  child?.kill("SIGTERM");
};
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
let started = false;
try {
  const dockerHost = await run([
    "docker",
    "context",
    "inspect",
    "--format",
    "{{.Endpoints.docker.Host}}",
  ]);
  if (!dockerHost.trim().startsWith("unix://")) {
    throw new Error("Refusing to start Scrabble on a remote Docker context");
  }
  if ((await run([...compose, "ps", "-q", "--status", "running"])).trim()) {
    throw new Error(
      "scrabble-local is already running; stop its existing launcher before restarting",
    );
  }
  // Refuse an occupied client port before starting persistent game services.
  const listener = createServer();
  await new Promise<void>((resolve, reject) => {
    listener.once("error", reject);
    listener.listen(5173, "127.0.0.1", resolve);
  });
  await new Promise<void>((resolve, reject) =>
    listener.close((error) => (error ? reject(error) : resolve())),
  );
  await mkdir(join(directory, "state"), { recursive: true, mode: 0o700 });
  const connection = z
    .object({
      endpoint: z.string().url(),
      certificateHash: z.array(z.int().min(0).max(255)).length(32),
    })
    .parse(JSON.parse(await run(["bun", "--no-env-file", "run", "dev:tls"])));
  started = true;
  console.log("Starting the local game-server with the prepared dictionary…");
  await run([...compose, "up", "--build", "--wait", "--wait-timeout", "60", "server"]);
  const url = new URL("http://localhost:5173/scrabble/");
  url.searchParams.set("server", connection.endpoint);
  url.searchParams.set("api", "http://127.0.0.1:8081");
  url.searchParams.set(
    "certificate",
    connection.certificateHash.map((byte) => byte.toString(16).padStart(2, "0")).join(""),
  );
  await Bun.write(join(directory, "url.txt"), `${url.href}\n`);
  child = Bun.spawn(
    [
      "bun",
      "--no-env-file",
      "run",
      "dev",
      "--",
      "--host",
      "127.0.0.1",
      "--port",
      "5173",
      "--strictPort",
    ],
    {
      cwd: root,
      env,
      stdout: "inherit",
      stderr: "inherit",
    },
  );
  console.log(
    `\nOpen this local game URL:\n${url.href}\n\nCtrl+C stops the server gracefully and preserves game state.`,
  );
  const code = await child.exited;
  child = undefined;
  if (!interrupted && code !== 0) {
    throw new Error("Local browser server exited unexpectedly");
  }
} finally {
  child?.kill("SIGTERM");
  if (child) {
    await child.exited;
  }
  if (started) {
    // Stop only this launcher's Compose project; keep TLS, dictionary and recovery storage.
    const shutdown = Bun.spawn([...compose, "stop", "server"], {
      cwd: root,
      env,
      stdout: "inherit",
      stderr: "inherit",
    });
    if ((await shutdown.exited) !== 0) {
      console.error(
        "Local server stop failed; inspect the scrabble-local Compose project before restarting.",
      );
      process.exitCode = 1;
    }
  }
  process.removeListener("SIGINT", stop);
  process.removeListener("SIGTERM", stop);
}
