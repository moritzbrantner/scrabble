import { type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { z } from "zod";
const path = process.env.SCRABBLE_GAME_FIXTURE;
if (path === undefined) {
  throw new Error("Use bun run test:browser to provision Scrabble");
}
export const gameFixture = z
  .strictObject({
    endpoint: z.string(),
    api: z.string(),
    status: z.string(),
    certificateHash: z.array(z.int().min(0).max(255)).length(32),
    opening: z.strictObject({
      requestId: z.string().regex(/^[0-9a-f]{32}$/),
      requestedAt: z.int().min(0),
    }),
  })
  .parse(JSON.parse(readFileSync(path, "utf8")));

export async function selectOpening(page: Page, opening = gameFixture.opening) {
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
