import { type Page } from "@playwright/test";

/** Change one real command payload; native authority and actual rejection delivery remain intact. */
export async function exchangeTwoOwnedTilesOnce(page: Page, tileIds: string[]) {
  if (tileIds.length !== 2 || tileIds[0] === tileIds[1]) {
    throw new Error("two distinct actually owned tiles are required");
  }
  await page.evaluate((owned) => {
    // oxlint-disable-next-line typescript/unbound-method -- Every call preserves the native writer receiver.
    const nativeWrite = WritableStreamDefaultWriter.prototype.write;
    let used = false;
    WritableStreamDefaultWriter.prototype.write = function (chunk: unknown) {
      if (!used && chunk instanceof Uint8Array && chunk.length >= 8 && chunk[1] === 1) {
        const envelope: unknown = JSON.parse(new TextDecoder().decode(chunk.subarray(8)));
        if (
          typeof envelope === "object" &&
          envelope !== null &&
          "command" in envelope &&
          typeof envelope.command === "object" &&
          envelope.command !== null &&
          "kind" in envelope.command &&
          envelope.command.kind === "exchange" &&
          "tile_ids" in envelope.command &&
          Array.isArray(envelope.command.tile_ids)
        ) {
          if (
            envelope.command.tile_ids.length !== 1 ||
            !owned.includes(envelope.command.tile_ids[0])
          ) {
            throw new Error("expected an actual one-tile exchange from this player's rack");
          }
          envelope.command.tile_ids = owned;
          const payload = new TextEncoder().encode(JSON.stringify(envelope));
          const changed = new Uint8Array(8 + payload.length);
          changed.set(chunk.subarray(0, 8));
          new DataView(changed.buffer).setUint16(6, payload.length);
          changed.set(payload, 8);
          used = true;
          return nativeWrite.call(this, changed);
        }
      }
      return nativeWrite.call(this, chunk);
    };
  }, tileIds);
}
