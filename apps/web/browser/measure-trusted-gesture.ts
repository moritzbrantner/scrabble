import { type Page } from "@playwright/test";

/** Measures trusted browser-dispatched gestures, not synthetic HTMLElement.click(). */
export type GestureTiming = {
  inputToDomMs: number;
  inputToFrameOpportunityMs: number;
  pointerType: string;
  trusted: boolean;
};

type TargetState =
  | { kind: "attribute"; name: string; value: string }
  | { kind: "class"; token: string; present: boolean };

type PendingGesture = {
  result: Promise<GestureTiming>;
  cancel: () => void;
};

export async function measureTrustedGesture(
  page: Page,
  options: {
    triggerSelector: string;
    observedSelector: string;
    expected: TargetState;
    input: "mouse" | "touch";
  },
): Promise<GestureTiming> {
  await page.evaluate(({ triggerSelector, observedSelector, expected }) => {
    const trigger = document.querySelector<HTMLElement>(triggerSelector);
    const observed = document.querySelector<HTMLElement>(observedSelector);
    if (trigger === null || observed === null) {
      throw new Error("Missing control for real-pointer performance test");
    }
    const matches = () =>
      expected.kind === "attribute"
        ? observed.getAttribute(expected.name) === expected.value
        : observed.classList.contains(expected.token) === expected.present;
    if (matches()) {
      throw new Error("The expected DOM transition is already satisfied");
    }

    let stop = () => {};
    const result = new Promise<GestureTiming>((resolve, reject) => {
      let pointer: { time: number; type: string; trusted: boolean } | undefined;
      let timer: number;
      const onPointerDown = (event: PointerEvent) => {
        if (event.target instanceof Node && trigger.contains(event.target) && !pointer) {
          pointer = {
            time: event.timeStamp,
            type: event.pointerType,
            trusted: event.isTrusted,
          };
        }
      };
      const observer = new MutationObserver(() => {
        if (!matches()) {
          return;
        }
        if (!pointer) {
          stop();
          reject(new Error("DOM changed without a matching pointerdown event"));
          return;
        }
        const observedPointer = pointer;
        const inputToDomMs = performance.now() - observedPointer.time;
        // A second frame callback occurs after at least one rendering opportunity,
        // but it cannot prove that physical display pixels were presented.
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            stop();
            resolve({
              inputToDomMs,
              inputToFrameOpportunityMs: performance.now() - observedPointer.time,
              pointerType: observedPointer.type,
              trusted: observedPointer.trusted,
            });
          });
        });
      });
      stop = () => {
        clearTimeout(timer);
        observer.disconnect();
        document.removeEventListener("pointerdown", onPointerDown, true);
      };
      document.addEventListener("pointerdown", onPointerDown, true);
      observer.observe(observed, {
        attributes: true,
        attributeFilter: [expected.kind === "attribute" ? expected.name : "class"],
      });
      timer = window.setTimeout(() => {
        stop();
        reject(new Error("Timed out waiting for a real-pointer DOM transition"));
      }, 8000);
    });
    // If Playwright's click fails before reading the result, do not leave a
    // browser-side unhandled rejection behind.
    void result.catch(() => {});
    (
      window as Window & { scrabblePendingGesture?: PendingGesture }
    ).scrabblePendingGesture = { result, cancel: stop };
  }, options);

  try {
    const button = page.locator(options.triggerSelector);
    if (options.input === "touch") {
      await button.tap();
    } else {
      await button.click();
    }
    const result = await page.evaluate(async () => {
      const pending = (window as Window & { scrabblePendingGesture?: PendingGesture })
        .scrabblePendingGesture;
      if (pending === undefined) {
        throw new Error("The trusted-gesture performance probe was lost");
      }
      return pending.result;
    });
    if (!result.trusted || result.pointerType !== options.input) {
      throw new Error(
        "Expected trusted " +
          options.input +
          " pointer input, got " +
          JSON.stringify({ type: result.pointerType, trusted: result.trusted }),
      );
    }
    if (
      !Number.isFinite(result.inputToDomMs) ||
      !Number.isFinite(result.inputToFrameOpportunityMs) ||
      result.inputToDomMs < 0 ||
      result.inputToFrameOpportunityMs < result.inputToDomMs
    ) {
      throw new Error("Invalid browser event timestamp or frame ordering");
    }
    return result;
  } finally {
    await page
      .evaluate(() => {
        const target = window as Window & { scrabblePendingGesture?: PendingGesture };
        target.scrabblePendingGesture?.cancel();
        delete target.scrabblePendingGesture;
      })
      .catch(() => {});
  }
}
