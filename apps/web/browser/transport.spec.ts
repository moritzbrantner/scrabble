import { readFileSync } from "node:fs";

import { expect, test } from "@playwright/test";
import { z } from "zod";
const path = process.env.SCRABBLE_TRANSPORT_FIXTURE;
if (path === undefined) {
  throw new Error("Use bun run test:browser to provision WebTransport");
}
const fixture = z
  .strictObject({
    endpoint: z.string(),
    matchId: z.string(),
    certificateHash: z.array(z.int().min(0).max(255)).length(32),
  })
  .parse(JSON.parse(readFileSync(path, "utf8")));

test("Chromium connects to the real runtime, sends commands/control, reassembles and reconnects privately", async ({
  page,
}) => {
  await page.goto("./");
  const result = await page.evaluate(async (fixture) => {
    // Vite serves source under its Pages base; the same module supplies the compile-time contract.
    const modulePath = "/scrabble/src/transport/browser-match.ts";
    const { BrowserMatch } = (await import(
      modulePath
    )) as typeof import("../src/transport/browser-match");
    const states: string[] = [];
    const admissions: { playerId: string; connectionEpoch: number }[] = [];
    const sequences: number[] = [];
    const snapshots: { tick: string; length: number }[] = [];
    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    const payloadSchema = (
      value: unknown,
    ): { version: number; sequence: number; padding: string } => {
      if (
        typeof value !== "object" ||
        value === null ||
        !("version" in value) ||
        !("sequence" in value) ||
        !("padding" in value) ||
        typeof value.version !== "number" ||
        typeof value.sequence !== "number" ||
        typeof value.padding !== "string"
      ) {
        throw new Error("Invalid transport fixture snapshot");
      }
      return { version: value.version, sequence: value.sequence, padding: value.padding };
    };
    const waitFor = async (condition: () => boolean) => {
      const deadline = Date.now() + 10_000;
      while (!condition()) {
        if (Date.now() > deadline) {
          throw new Error(`Timed out; states: ${states.join(",")}`);
        }
        await new Promise<void>((resolve) => setTimeout(resolve, 10));
      }
    };
    const client = new BrowserMatch({
      endpoint: fixture.endpoint,
      matchId: fixture.matchId,
      serverCertificateHashes: [
        { algorithm: "sha-256", value: new Uint8Array(fixture.certificateHash).buffer },
      ],
      onState: (state) => {
        states.push(state.kind);
        if (state.kind === "connected") {
          admissions.push(state.admission);
        }
      },
      onSnapshot: (snapshot) => {
        const payload = payloadSchema(JSON.parse(decoder.decode(snapshot.payload)));
        sequences.push(payload.sequence);
        snapshots.push({ tick: String(snapshot.tick), length: snapshot.payload.length });
      },
    });
    let run = client.run();
    try {
      await waitFor(() => admissions.length === 1 && snapshots.length > 0);
      const accepted = await client.control(encoder.encode("ping"));
      const rejected = await client.control(encoder.encode("reject"));
      const delayed = Array.from({ length: 4 }, () => client.control(encoder.encode("delay")));
      let concurrencyError = "";
      try {
        await client.control(encoder.encode("ping"));
      } catch (error) {
        concurrencyError = error instanceof Error ? error.message : "unknown";
      }
      await Promise.all(delayed);
      const beforeStall = snapshots.length;
      const stalled = client.control(encoder.encode("stall")).then(
        () => "unexpected success",
        (error: unknown) => (error instanceof Error ? error.message : "unknown"),
      );
      await waitFor(() => snapshots.length > beforeStall + 2);
      const timeoutError = await stalled;
      const first = await client.sendCommand(() => encoder.encode("fixture-command"));
      await waitFor(() => sequences.includes(first));
      client.disconnect();
      await run;
      run = client.run();
      await waitFor(() => admissions.length === 2);
      const second = await client.sendCommand(() => encoder.encode("fixture-command"));
      await waitFor(() => sequences.includes(second));
      return {
        concurrencyError,
        timeoutError,
        states,
        admissions,
        snapshots,
        first,
        second,
        control: decoder.decode(accepted.payload),
        accepted: accepted.accepted,
        rejected: rejected.accepted,
        serializedClient: JSON.stringify(client),
        currentUrl: location.href,
        storage: [localStorage.length, sessionStorage.length],
      };
    } finally {
      client.close();
      await run;
    }
  }, fixture);
  expect(result.concurrencyError).toContain("Too many control");
  expect(result.timeoutError).toContain("failed or timed out");
  expect(result.admissions.map((value) => value.playerId)).toEqual(["1", "1"]);
  expect(result.admissions.map((value) => value.connectionEpoch)).toEqual([1, 2]);
  expect(result.first).toBe(1);
  expect(result.second).toBe(2);
  expect(result.accepted).toBe(true);
  expect(result.rejected).toBe(false);
  expect(result.control).toBe("pong:1:1");
  expect(result.states).toContain("disconnected");
  expect(result.snapshots.every((value) => value.length > 12_000)).toBe(true);
  expect(result.serializedClient).toBe("{}");
  expect(result.currentUrl).not.toContain("reconnect");
  expect(result.storage).toEqual([0, 0]);
});
