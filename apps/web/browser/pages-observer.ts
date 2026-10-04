import { expect, type Page } from "@playwright/test";
import { playerSnapshot, type PlayerSnapshot } from "../src/public-state";
import { SnapshotReassembler } from "../src/transport/wire";

declare global {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- Browser binding requires Window declaration merging.
  interface Window {
    inspectScrabbleDatagram: (bytes: number[]) => Promise<void>;
  }
}

/** Inspect actual datagrams without modifying the built application's projections. */
export async function observeSnapshots(page: Page, recipient: string, board = false) {
  let latest: PlayerSnapshot | undefined;
  const fragments = new SnapshotReassembler();
  await page.exposeBinding("inspectScrabbleDatagram", (_source, bytes: number[]) => {
    const frame = fragments.accept(Uint8Array.from(bytes));
    if (frame !== undefined) {
      const parsed = playerSnapshot.safeParse(JSON.parse(new TextDecoder().decode(frame.payload)));
      expect(parsed.success).toBe(true);
      if (!parsed.success) {
        return;
      }
      expect(parsed.data.own_rack.player_id).toBe(recipient);
      if (board) {
        expect(parsed.data.own_rack.tiles.length).toBe(0);
      }
      latest = parsed.data;
    }
  });
  await page.addInitScript(() => {
    const NativeTransport = WebTransport;
    window.WebTransport = class extends NativeTransport {
      constructor(url: string | URL, options?: WebTransportOptions) {
        super(url, options);
        const reader = this.datagrams.readable.getReader();
        const readable = new ReadableStream<Uint8Array>({
          async pull(controller) {
            const result = await reader.read();
            if (result.done) {
              controller.close();
              return;
            }
            await window.inspectScrabbleDatagram(Array.from(result.value));
            controller.enqueue(result.value);
          },
          cancel(reason) {
            return reader.cancel(reason);
          },
        });
        Object.defineProperty(this.datagrams, "readable", { value: readable });
      }
    };
  });
  return () => latest;
}
