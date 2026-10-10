import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname } from "node:path";

import { type Page } from "@playwright/test";
import { z } from "zod";
const path = process.env.SCRABBLE_GAME_FIXTURE;
if (path === undefined) {
  throw new Error("Use bun run test:browser to provision Scrabble");
}
const openingSchema = z.strictObject({
  requestId: z.string().regex(/^[0-9a-f]{32}$/),
  requestedAt: z.int().min(0),
});
export const gameFixture = z
  .strictObject({
    endpoint: z.string(),
    api: z.string(),
    status: z.string(),
    certificateHash: z.array(z.int().min(0).max(255)).length(32),
    opening: openingSchema,
    reconnectOpening: openingSchema,
  })
  .parse(JSON.parse(readFileSync(path, "utf8")));

const directory = dirname(path);
export async function selectOpening(page: Page, candidate = gameFixture.opening) {
  // Startup deals are derived from timestamped IDs. Slow suites must select a fresh native
  // deal rather than extending the production creation window or changing its identity.
  const opening =
    Date.now() - candidate.requestedAt * 1000 >= 60_000
      ? openingSchema.parse(
          JSON.parse(
            execFileSync(
              "target/debug/examples/browser_game_fixture",
              ["--fresh-opening", directory, candidate.requestId],
              { encoding: "utf8", timeout: 10_000 },
            ),
          ),
        )
      : candidate;
  await page.evaluate(({ requestId, requestedAt }) => {
    const original = crypto.getRandomValues.bind(crypto);
    const originalNow = Date.now;
    crypto.getRandomValues = <T extends ArrayBufferView | null>(array: T): T => {
      if (array instanceof Uint8Array && array.length === 16) {
        array.set(Uint8Array.from(requestId.match(/../g) ?? [], (hex) => Number.parseInt(hex, 16)));
        crypto.getRandomValues = original;
        Date.now = () => {
          Date.now = originalNow;
          return requestedAt * 1000;
        };
        return array;
      }
      return original(array);
    };
  }, opening);
}
