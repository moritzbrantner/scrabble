import { type Locator, type Page } from "@playwright/test";

async function centre(locator: Locator) {
  const box = await locator.boundingBox();
  if (box === null) {
    throw new Error("Drag endpoint is not rendered");
  }
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

/** Real trusted mouse pointer drag between the centres of two elements. */
export async function mouseDrag(page: Page, from: Locator, to: Locator) {
  const start = await centre(from);
  const end = await centre(to);
  await page.mouse.move(start.x, start.y);
  await page.mouse.down();
  await page.mouse.move(start.x + 12, start.y + 4, { steps: 2 });
  await page.mouse.move(end.x, end.y, { steps: 6 });
  await page.mouse.up();
}

/** Trusted touch drag through Chromium's input pipeline (needs a `hasTouch` context). */
export async function touchDrag(page: Page, from: Locator, to: Locator, cancel = false) {
  const start = await centre(from);
  const end = await centre(to);
  const cdp = await page.context().newCDPSession(page);
  const touch = (
    type: "touchStart" | "touchMove" | "touchEnd" | "touchCancel",
    x: number,
    y: number,
  ) =>
    cdp.send("Input.dispatchTouchEvent", {
      type,
      touchPoints: type === "touchEnd" || type === "touchCancel" ? [] : [{ x, y }],
    });
  await touch("touchStart", start.x, start.y);
  for (let step = 1; step <= 6; step += 1) {
    await touch(
      "touchMove",
      start.x + ((end.x - start.x) * step) / 6,
      start.y + ((end.y - start.y) * step) / 6,
    );
  }
  await touch(cancel ? "touchCancel" : "touchEnd", end.x, end.y);
  await cdp.detach();
}
