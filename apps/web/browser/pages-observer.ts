import { expect, type Page } from "@playwright/test";
import { playerSnapshot, type PlayerSnapshot } from "../src/public-state";
import { SnapshotReassembler } from "../src/transport/wire";

declare global {
  // oxlint-disable-next-line typescript/consistent-type-definitions -- Browser binding requires Window declaration merging.
  interface Window {
    inspectScrabbleDatagrams: (datagrams: string[]) => Promise<void>;
  }
}

/** Inspect actual datagrams without modifying the built application's projections. */
export async function observeSnapshots(
  page: Page,
  recipient: string,
  board = false,
  holdRestoredSnapshot = false,
) {
  let latest: PlayerSnapshot | undefined;
  let inspectedPayload: string | undefined;
  const fragments = new SnapshotReassembler();
  await page.exposeBinding("inspectScrabbleDatagrams", (_source, datagrams: string[]) => {
    for (const encoded of datagrams) {
      const frame = fragments.accept(Buffer.from(encoded, "base64"));
      if (frame !== undefined) {
        const payload = new TextDecoder().decode(frame.payload);
        // Native ticks repeat unchanged projections. Validate each distinct projection
        // once rather than flooding Playwright traces with duplicate assertions.
        if (payload === inspectedPayload) {
          continue;
        }
        const parsed = playerSnapshot.safeParse(JSON.parse(payload));
        expect(parsed.success).toBe(true);
        if (!parsed.success) {
          return;
        }
        expect(parsed.data.own_rack.player_id).toBe(recipient);
        if (board) {
          expect(parsed.data.own_rack.tiles.length).toBe(0);
        }
        latest = parsed.data;
        inspectedPayload = payload;
      }
    }
  });
  await page.addInitScript((hold) => {
    const NativeTransport = WebTransport;
    let connections = 0;
    window.WebTransport = class extends NativeTransport {
      constructor(url: string | URL, options?: WebTransportOptions) {
        super(url, options);
        connections += 1;
        const released = new Promise<void>((resolve) => {
          if (!hold || connections !== 2) {
            resolve();
            return;
          }
          const release = () => resolve();
          window.addEventListener("test-release-snapshot", release, { once: true });
          this.closed.then(release, release);
        });
        const reader = this.datagrams.readable.getReader();
        const inspection: string[] = [];
        let inspecting = false;
        let cancelled = false;
        const inspect = async () => {
          if (inspecting || cancelled) {
            return;
          }
          inspecting = true;
          try {
            while (inspection.length > 0 && !cancelled) {
              // Batch at most ten inspections per second; gameplay still reads every datagram.
              await new Promise<void>((resolve) => setTimeout(resolve, 100));
              if (cancelled) {
                return;
              }
              await window.inspectScrabbleDatagrams(inspection.splice(0));
            }
          } finally {
            inspecting = false;
          }
        };
        const readable = new ReadableStream<Uint8Array>({
          async pull(controller) {
            const result = await reader.read();
            await released;
            if (result.done) {
              controller.close();
              return;
            }
            // Drain QUIC independently of Node inspection, with one binding in flight.
            // A bounded queue drops old observations under load, just like native datagrams.
            // Base64 preserves the exact bytes without tracing thousands of numeric arguments.
            inspection.push(btoa(String.fromCharCode(...result.value)));
            if (inspection.length > 128) {
              inspection.shift();
            }
            void inspect().catch(() => {
              if (!cancelled) {
                controller.error(new Error("Snapshot observation failed"));
              }
            });
            controller.enqueue(result.value);
          },
          cancel(reason) {
            cancelled = true;
            inspection.length = 0;
            return reader.cancel(reason);
          },
        });
        Object.defineProperty(this.datagrams, "readable", { value: readable });
      }
    };
  }, holdRestoredSnapshot);
  return () => latest;
}
