import { createDecipheriv, createECDH, hkdfSync, randomBytes } from "node:crypto";
import { createServer } from "node:http";
import { type AddressInfo } from "node:net";

import { expect, test } from "./fixtures";

import { freshOpening, gameFixture, selectOpening } from "./game-fixture";
import { observeSnapshots } from "./pages-observer";

type Received = {
  path: string;
  headers: Record<string, string | string[] | undefined>;
  body: Buffer;
};

/** RFC 8291 receiver, independent of the server implementation. */
function decrypt(privateKey: ReturnType<typeof createECDH>, auth: Buffer, body: Buffer): string {
  const salt = body.subarray(0, 16);
  const idLength = body[20] ?? 0;
  const sender = body.subarray(21, 21 + idLength);
  const receiver = privateKey.getPublicKey();
  const shared = privateKey.computeSecret(sender);
  const info = Buffer.concat([Buffer.from("WebPush: info\0"), receiver, sender]);
  const ikm = Buffer.from(hkdfSync("sha256", shared, auth, info, 32));
  const cek = Buffer.from(hkdfSync("sha256", ikm, salt, "Content-Encoding: aes128gcm\0", 16));
  const nonce = Buffer.from(hkdfSync("sha256", ikm, salt, "Content-Encoding: nonce\0", 12));
  const record = body.subarray(21 + idLength);
  const decipher = createDecipheriv("aes-128-gcm", cek, nonce);
  decipher.setAuthTag(record.subarray(record.length - 16));
  const plain = Buffer.concat([
    decipher.update(record.subarray(0, record.length - 16)),
    decipher.final(),
  ]);
  expect(plain.at(-1)).toBe(2);
  return plain.subarray(0, -1).toString();
}

test("an opted-in player gets one encrypted turn push after closing the tab; others get none", async ({
  page,
  browser,
}) => {
  test.setTimeout(60_000);
  const received: Received[] = [];
  const service = createServer((request, response) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => {
      received.push({
        path: request.url ?? "",
        headers: request.headers,
        body: Buffer.concat(chunks),
      });
      response.writeHead(201).end();
    });
  });
  await new Promise<void>((resolve) => service.listen(0, "127.0.0.1", resolve));
  const origin = `http://127.0.0.1:${(service.address() as AddressInfo).port}`;

  // Headless Chromium has no real push service: stand in for PushManager with a device key
  // pair this test holds, so the native server's encrypted delivery can be checked end to end.
  const adaKey = createECDH("prime256v1");
  adaKey.generateKeys();
  const adaAuth = randomBytes(16);
  const phones = await browser.newContext({ viewport: { width: 390, height: 844 } });
  await phones.grantPermissions(["notifications"]);
  const ada = await phones.newPage();
  const lin = await phones.newPage();
  await ada.addInitScript(
    ({ endpoint, p256dh, auth }) => {
      const bytes = (text: string) =>
        Uint8Array.from(atob(text), (char) => char.charCodeAt(0)).buffer;
      let current: PushSubscription | null = null;
      PushManager.prototype.getSubscription = async () => current;
      PushManager.prototype.subscribe = async (options?: PushSubscriptionOptionsInit) => {
        current = {
          endpoint,
          expirationTime: null,
          options: {
            applicationServerKey: options?.applicationServerKey ?? null,
            userVisibleOnly: true,
          },
          getKey: (name: PushEncryptionKeyName) => bytes(name === "p256dh" ? p256dh : auth),
          unsubscribe: async () => {
            current = null;
            return true;
          },
          toJSON: () => ({ endpoint }),
        } as unknown as PushSubscription;
        return current;
      };
    },
    {
      endpoint: `${origin}/push/ada-phone`,
      p256dh: adaKey.getPublicKey().toString("base64"),
      auth: adaAuth.toString("base64"),
    },
  );
  const boardState = await observeSnapshots(page, "1", true);
  const adaState = await observeSnapshots(ada, "2");
  const linState = await observeSnapshots(lin, "3");
  const certificate = gameFixture.certificateHash
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  try {
    await page.goto(
      `./?server=${encodeURIComponent(gameFixture.endpoint)}&api=${encodeURIComponent(gameFixture.api)}&certificate=${certificate}`,
    );
    await selectOpening(page, freshOpening(gameFixture.opening.requestId));
    await page.getByRole("button", { name: "Create game", exact: true }).click();
    const href = await page
      .getByRole("link", { name: "Join this game", exact: true })
      .getAttribute("href");
    for (const [phone, name] of [
      [ada, "Ada"],
      [lin, "Lin"],
    ] as const) {
      await phone.goto(href ?? "");
      await phone.getByLabel("Player name", { exact: true }).fill(name);
      await phone.getByRole("button", { name: "Join game", exact: true }).click();
      await expect(phone.getByRole("heading", { name, exact: true })).toBeVisible();
    }
    await page.getByRole("button", { name: "Start game", exact: true }).click();
    await expect.poll(() => adaState()?.own_rack.tiles.length).toBe(7);
    await expect.poll(() => linState()?.own_rack.tiles.length).toBe(7);

    // Ada deliberately opts in; Lin does not. Nothing is sent for a turn already under way.
    const toggle = ada.getByRole("button", { name: "Notify me when it is my turn", exact: true });
    await toggle.click();
    await expect(
      ada.getByText("Turn notifications are on for this device.", { exact: true }),
    ).toBeVisible();
    await expect(
      ada.getByRole("button", { name: "Stop turn notifications on this device", exact: true }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      await ada.evaluate(
        async () => (await navigator.serviceWorker.getRegistration()) !== undefined,
      ),
    ).toBe(true);
    await expect(
      lin.getByRole("button", { name: "Notify me when it is my turn", exact: true }),
    ).toHaveAttribute("aria-pressed", "false");

    // Ada plays AT; it becomes Lin's turn: no push for anyone (Lin never opted in).
    const adaRack = ada.getByRole("list", { name: "Your rack", exact: true });
    const editor = ada.getByRole("region", { name: "Move editor", exact: true });
    for (const [index, letter] of ["A", "T"].entries()) {
      await adaRack
        .getByRole("button", { name: new RegExp(`^${letter},`) })
        .first()
        .click();
      await editor
        .getByRole("button", { name: new RegExp(`^Row 8, column ${8 + index}:`) })
        .click();
    }
    await editor.getByRole("button", { name: "Commit move", exact: true }).click();
    await expect.poll(() => boardState()?.public.board.length).toBe(2);
    await expect(lin.getByText("It is your turn.", { exact: true })).toBeVisible();
    await lin.waitForTimeout(2_500);
    expect(received).toHaveLength(0);

    // Ada closes her tab. Lin passes; Ada's authoritative turn begins and she is notified.
    await ada.close();
    await lin.getByRole("button", { name: "Pass turn", exact: true }).click();
    await lin.getByRole("button", { name: "Confirm pass", exact: true }).click();
    await expect.poll(() => received.length, { timeout: 10_000 }).toBe(1);
    const [push] = received;
    expect(push?.path).toBe("/push/ada-phone");
    expect(push?.headers["content-encoding"]).toBe("aes128gcm");
    expect(String(push?.headers["authorization"])).toMatch(
      /^vapid t=[\w-]+\.[\w-]+\.[\w-]+, k=[\w-]+$/,
    );
    expect(decrypt(adaKey, adaAuth, push?.body ?? Buffer.alloc(0))).toBe('{"kind":"turn"}');
    // The shared board and Lin never get turn notifications; no duplicates follow.
    await lin.waitForTimeout(2_500);
    expect(received).toHaveLength(1);
  } finally {
    await phones.close();
    service.close();
  }
});
