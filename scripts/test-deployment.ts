import { mkdtemp, mkdir, copyFile, readFile, rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { decodeJoinInformation, newCreateRequest } from "../apps/web/src/create-game";

async function run(command: string[], env: NodeJS.ProcessEnv = process.env): Promise<string> {
  const child = Bun.spawn(command, { env, stdout: "pipe", stderr: "pipe" });
  const [stdout, , code] = await Promise.all([
    new Response(child.stdout).text(),
    new Response(child.stderr).text(),
    child.exited,
  ]);
  if (code !== 0) {
    throw new Error(`Deployment check command failed: ${command[0]}`);
  }
  return stdout;
}
const dockerHost = await run([
  "docker",
  "context",
  "inspect",
  "--format",
  "{{.Endpoints.docker.Host}}",
]);
if (
  !dockerHost.trim().startsWith("unix://") ||
  process.env.DOCKER_HOST ||
  process.env.DOCKER_CONTEXT
) {
  throw new Error("Deployment checks require the default local Unix Docker context");
}
const uid = process.getuid?.();
const gid = process.getgid?.();
if (uid === undefined || gid === undefined) {
  throw new Error("Deployment checks require Unix process ownership");
}
async function port(): Promise<number> {
  const listener = createServer();
  await new Promise<void>((resolve, reject) => {
    listener.once("error", reject);
    listener.listen(0, "127.0.0.1", resolve);
  });
  const address = listener.address();
  if (address === null || typeof address === "string") {
    throw new Error("Missing deployment check port");
  }
  await new Promise<void>((resolve, reject) =>
    listener.close((error) => (error ? reject(error) : resolve())),
  );
  return address.port;
}
const directory = await mkdtemp(join(tmpdir(), "scrabble-deployment-"));
const env: NodeJS.ProcessEnv = Object.fromEntries(
  Object.entries(process.env).filter(
    ([key]) => !key.startsWith("SCRABBLE_") && !key.startsWith("DEPLOY_"),
  ),
);
const tls = join(directory, "tls");
const state = join(directory, "state");
const content = join(directory, "content");
const httpsPort = await port();
const quicPort = await port();
Object.assign(env, {
  DEPLOY_TLS_DIRECTORY: tls,
  DEPLOY_STATE_DIRECTORY: state,
  DEPLOY_CONTENT_DIRECTORY: content,
  DEPLOY_UID: String(uid),
  DEPLOY_GID: String(gid),
  DEPLOY_HTTPS_PORT: `127.0.0.1:${httpsPort}`,
  DEPLOY_QUIC_PORT: `127.0.0.1:${quicPort}`,
  DEPLOY_VERSION: "deployment-check",
  DEPLOY_DICTIONARY_NAME: "scrabble-authored-fixture",
  DEPLOY_DICTIONARY_REVISION: "1",
  DEPLOY_BOARD_ORIGIN: "https://moritzbrantner.github.io",
});
const compose = [
  "docker",
  "compose",
  "--env-file",
  "/dev/null",
  "-p",
  `scrabble-check-${crypto.randomUUID()}`,
  "-f",
  "compose.yaml",
  "--profile",
  "production",
];
async function services(...args: string[]) {
  return run([...compose, ...args], env);
}
try {
  for (const path of [tls, state, content]) {
    await mkdir(path, { mode: 0o700 });
  }
  await copyFile("crates/scrabble-game/fixtures/words.txt", join(content, "words.txt"));
  await run(["cargo", "run", "-q", "-p", "scrabble-server", "--example", "local_tls", "--locked"], {
    ...env,
    SCRABBLE_MODE: "local",
    SCRABBLE_CERT_PEM: join(tls, "cert.pem"),
    SCRABBLE_KEY_PEM: join(tls, "key.pem"),
  });
  await services("build", "server");
  await services("up", "-d", "--wait", "--wait-timeout", "45");
  async function request(path: string, status: number, args: string[] = []): Promise<string> {
    const result = await run([
      "curl",
      "--silent",
      "--show-error",
      "--max-time",
      "10",
      "--cacert",
      join(tls, "cert.pem"),
      "--write-out",
      "\n%{http_code}",
      ...args,
      `https://localhost:${httpsPort}${path}`,
    ]);
    const separator = result.lastIndexOf("\n");
    if (Number(result.slice(separator + 1)) !== status) {
      throw new Error(`Unexpected deployment response: ${path}`);
    }
    return result.slice(0, separator);
  }
  await request("/healthz", 200);
  await request("/readyz", 200);
  await request("/status", 404);
  await request("/matches/example/status", 404);
  await request("/readyz", 403, ["--request", "POST"]);
  const requestFile = join(directory, "request.json");
  await Bun.write(requestFile, JSON.stringify(newCreateRequest()), { mode: 0o600 });
  const creation = [
    "--request",
    "POST",
    "--header",
    "Content-Type: application/json",
    "--data-binary",
    `@${requestFile}`,
  ];
  await request("/api/games", 403, [...creation, "--header", "Origin: https://wrong.example"]);
  const created = decodeJoinInformation(
    JSON.parse(
      await request("/api/games", 200, [
        ...creation,
        "--header",
        "Origin: https://moritzbrantner.github.io",
      ]),
    ),
    "https://localhost/game",
  );
  const metadataFile = join(directory, "join.json");
  await Bun.write(metadataFile, JSON.stringify(created));
  await run(
    [
      "cargo",
      "run",
      "-q",
      "-p",
      "scrabble-server",
      "--example",
      "deployment_probe",
      "--locked",
      "--",
      join(tls, "cert.pem"),
      `https://127.0.0.1:${quicPort}${created.matchPath}`,
      metadataFile,
    ],
    env,
  );
  const seed = await readFile(join(state, "seed"));
  await services("stop", "gateway", "server");
  await services(
    "up",
    "-d",
    "--wait",
    "--wait-timeout",
    "45",
    "--force-recreate",
    "server",
    "gateway",
  );
  const restored = decodeJoinInformation(
    JSON.parse(await request(`/api/games/${created.matchId}`, 200)),
    "https://localhost/game",
  );
  if (
    JSON.stringify(restored) !== JSON.stringify(created) ||
    !seed.equals(await readFile(join(state, "seed")))
  ) {
    throw new Error("Deployment restart failed to preserve match and initialization identity");
  }
  console.log(
    "Deployment HTTPS routing, origin rejection, private management routes, and graceful match recovery passed",
  );
} finally {
  try {
    await services("down", "--timeout", "30");
  } finally {
    await rm(directory, { recursive: true });
  }
}
