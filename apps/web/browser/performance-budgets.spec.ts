import { execFileSync } from "node:child_process";
import { arch, cpus, platform, release, totalmem } from "node:os";

import { expect, test, type Browser, type Page, type TestInfo } from "@playwright/test";

import authoredRack from "../src/fixtures/private-rack.json" with { type: "json" };
import { playerSnapshot } from "../src/public-state";
import { measureTrustedGesture } from "./measure-trusted-gesture";
import { type BoardMeasurements, type DraftMeasurements } from "./performance-fixture";
import { type BrowserWorkload } from "./populated-performance-state";

type PerformanceApi = {
  measureBoardRefreshes: (
    workload?: BrowserWorkload,
    includeFrame?: boolean,
  ) => Promise<BoardMeasurements>;
  measureWordDraftInteractions: (
    workload?: BrowserWorkload,
    includeFrame?: boolean,
  ) => Promise<DraftMeasurements>;
};

// This is an interaction budget, not Playwright's test timeout or a 60 FPS claim.
const INTERACTION_BUDGET_MS = 50;
// Shared CI is advisory. Only enforce on an identified, controlled target runner.
const enforceTiming = process.env.SCRABBLE_PERF_ENFORCE === "1";

function enforceBudget(observed: number, label: string) {
  if (enforceTiming) {
    expect(observed, label).toBeLessThanOrEqual(INTERACTION_BUDGET_MS);
  }
}

function summary(samples: number[]) {
  expect(samples.length).toBeGreaterThanOrEqual(12);
  expect(samples.every((sample) => Number.isFinite(sample) && sample >= 0)).toBe(true);
  const sorted = [...samples].sort((left, right) => left - right);
  const percentile = (fraction: number) => sorted[Math.ceil(sorted.length * fraction) - 1]!;
  return {
    count: samples.length,
    minMs: sorted[0]!,
    p50Ms: percentile(0.5),
    p95Ms: percentile(0.95),
    p99Ms: percentile(0.99),
    maxMs: sorted[sorted.length - 1]!,
    samplesMs: samples, // retain original observation order and full precision
  };
}

async function recordEvidence(
  page: Page,
  browser: Browser,
  testInfo: TestInfo,
  workload: string,
  measurements: Record<string, number[]>,
  boundary = "synthetic HTMLElement.click -> React commit/layout; optional second rAF after >=1 frame opportunity; no verified physical paint",
) {
  const browserEnvironment = await page.evaluate(() => ({
    userAgent: navigator.userAgent,
    language: navigator.language,
    viewport: { width: innerWidth, height: innerHeight },
    devicePixelRatio: devicePixelRatio,
    maxTouchPoints: navigator.maxTouchPoints,
    hardwareConcurrency: navigator.hardwareConcurrency,
    visibilityState: document.visibilityState,
    reducedMotion: matchMedia("(prefers-reduced-motion: reduce)").matches,
  }));
  expect(browserEnvironment.visibilityState).toBe("visible");
  const cpu = cpus()[0];
  const report = {
    workload,
    sourceRevision: execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim(),
    build: "optimized Vite --mode test; served through vite preview (--pages)",
    measuredBoundary: boundary,
    budgetMs: INTERACTION_BUDGET_MS,
    enforcement: enforceTiming ? "opt-in strict" : "advisory shared-runner",
    target: process.env.SCRABBLE_PERF_TARGET ?? null,
    githubRunId: process.env.GITHUB_RUN_ID ?? null,
    browser: { project: testInfo.project.name, version: browser.version(), ...browserEnvironment },
    host: {
      platform: platform(),
      arch: arch(),
      release: release(),
      cpuModel: cpu?.model ?? null,
      logicalCpuCount: cpus().length,
      totalMemoryBytes: totalmem(),
    },
    metrics: Object.fromEntries(
      Object.entries(measurements).map(([name, samples]) => [name, summary(samples)]),
    ),
  };
  const text = JSON.stringify(report);
  console.log("SCRABBLE_PERF_EVIDENCE " + text);
  await testInfo.attach("scrabble-performance-evidence", {
    body: Buffer.from(JSON.stringify(report, null, 2)),
    contentType: "application/json",
  });
}

async function prepare(page: Page) {
  await page.goto("browser/performance.html");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
}

test("50 ms p95: rendering changed Scrabble board snapshots", async ({
  page,
  browser,
}, testInfo) => {
  await prepare(page);
  const { layoutMs } = await page.evaluate(async () => {
    const fixture = (window as Window & { scrabblePerformance?: PerformanceApi })
      .scrabblePerformance;
    if (fixture === undefined) {
      throw new Error("Optimized board performance fixture is missing");
    }
    return fixture.measureBoardRefreshes();
  });
  const observed = summary(layoutMs).p95Ms;
  console.log("Scrabble board React refresh: p95=" + observed.toFixed(2) + "ms, budget=50ms");
  await recordEvidence(page, browser, testInfo, "opening board 225 cells", { layoutMs });
  enforceBudget(observed, "React reconcile, DOM commit and layout of the shared board");
});

test("50 ms p95: placing word tiles and clearing a move draft", async ({
  page,
  browser,
}, testInfo) => {
  await prepare(page);
  const { placeMs, resetMs } = await page.evaluate(async () => {
    const fixture = (window as Window & { scrabblePerformance?: PerformanceApi })
      .scrabblePerformance;
    if (fixture === undefined) {
      throw new Error("Optimized draft performance fixture is missing");
    }
    return fixture.measureWordDraftInteractions();
  });
  const placement = summary(placeMs).p95Ms;
  const reset = summary(resetMs).p95Ms;
  console.log(
    "Scrabble word-draft placement: p95=" +
      placement.toFixed(2) +
      "ms; reset: p95=" +
      reset.toFixed(2) +
      "ms; budget=50ms",
  );
  await recordEvidence(page, browser, testInfo, "opening AH draft, 225 cells", {
    placeLayoutMs: placeMs,
    resetLayoutMs: resetMs,
  });
  enforceBudget(placement, "Select rack tile -> place square -> updated editor DOM/layout");
  enforceBudget(reset, "Cancel draft -> editor DOM restored");
});

test("50 ms p95: populated board refresh and draft reach a frame opportunity", async ({
  page,
  browser,
}, testInfo) => {
  await prepare(page);
  const result = await page.evaluate(async () => {
    const fixture = (window as Window & { scrabblePerformance?: PerformanceApi })
      .scrabblePerformance;
    if (fixture === undefined) {
      throw new Error("Optimized performance fixture is missing");
    }
    return {
      board: await fixture.measureBoardRefreshes("populated", true),
      draft: await fixture.measureWordDraftInteractions("populated", true),
    };
  });
  const measurements = {
    boardLayoutMs: result.board.layoutMs,
    boardFrameOpportunityMs: result.board.frameOpportunityMs,
    placeLayoutMs: result.draft.placeMs,
    placeFrameOpportunityMs: result.draft.placeFrameOpportunityMs,
    resetLayoutMs: result.draft.resetMs,
    resetFrameOpportunityMs: result.draft.resetFrameOpportunityMs,
  };
  await recordEvidence(
    page,
    browser,
    testInfo,
    "connected 81/82 tiles, 225 cells; AH edit",
    measurements,
  );
  for (const [name, samples] of Object.entries(measurements)) {
    enforceBudget(summary(samples).p95Ms, "Populated desktop: " + name);
  }
});

test.describe("phone-sized touch Chromium emulation (not a physical phone)", () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });

  test("50 ms p95: populated draft reaches a frame opportunity", async ({
    page,
    browser,
  }, testInfo) => {
    await prepare(page);
    const draft = await page.evaluate(async () => {
      const fixture = (window as Window & { scrabblePerformance?: PerformanceApi })
        .scrabblePerformance;
      if (fixture === undefined) {
        throw new Error("Optimized performance fixture is missing");
      }
      return fixture.measureWordDraftInteractions("populated", true);
    });
    const measurements = {
      placeLayoutMs: draft.placeMs,
      placeFrameOpportunityMs: draft.placeFrameOpportunityMs,
      resetLayoutMs: draft.resetMs,
      resetFrameOpportunityMs: draft.resetFrameOpportunityMs,
    };
    await recordEvidence(
      page,
      browser,
      testInfo,
      "connected 81 tiles, 225 cells; AH edit; phone viewport/touch emulation",
      measurements,
    );
    for (const [name, samples] of Object.entries(measurements)) {
      enforceBudget(summary(samples).p95Ms, "Emulated phone: " + name);
    }
  });
});

async function verifyTrustedPopulatedGestures(
  page: Page,
  browser: Browser,
  testInfo: TestInfo,
  input: "mouse" | "touch",
) {
  testInfo.setTimeout(60_000);
  await prepare(page);
  await page.evaluate(() => {
    const fixture = (
      window as Window & {
        scrabblePerformance?: PerformanceApi & { showPopulatedEditor: () => void };
      }
    ).scrabblePerformance;
    if (fixture === undefined) {
      throw new Error("Optimized editor performance fixture is missing");
    }
    fixture.showPopulatedEditor();
  });

  const editor = page.getByRole("region", { name: "Move editor", exact: true });
  await expect(editor).toBeVisible();
  await expect(editor.locator(".phone-board-grid button")).toHaveCount(225);
  await expect(editor.locator(".committed-square")).toHaveCount(81);

  const ownRack = playerSnapshot.parse(authoredRack.snapshots.playing).own_rack.tiles;
  const findTile = (letter: "A" | "H") => {
    const tile = ownRack.find(
      (candidate) => candidate.face.kind === "letter" && candidate.face.letter === letter,
    );
    if (tile === undefined) {
      throw new Error("Missing authored tile " + letter);
    }
    return tile.id;
  };
  const positions = [
    { tileId: findTile("A"), square: 118 },
    { tileId: findTile("H"), square: 119 },
  ] as const;
  const samples = {
    selectInputToDomMs: [] as number[],
    selectInputToFrameOpportunityMs: [] as number[],
    placeInputToDomMs: [] as number[],
    placeInputToFrameOpportunityMs: [] as number[],
    resetInputToDomMs: [] as number[],
    resetInputToFrameOpportunityMs: [] as number[],
  };

  for (let repetition = 0; repetition < 12; repetition++) {
    for (const { tileId, square } of positions) {
      const rackSelector = '.phone-rack [data-tile-id="' + tileId + '"] button';
      const squareSelector = '[data-square-index="' + square + '"]';
      const selected = await measureTrustedGesture(page, {
        triggerSelector: rackSelector,
        observedSelector: rackSelector,
        expected: { kind: "attribute", name: "aria-pressed", value: "true" },
        input,
      });
      samples.selectInputToDomMs.push(selected.inputToDomMs);
      samples.selectInputToFrameOpportunityMs.push(selected.inputToFrameOpportunityMs);
      await expect(editor.locator(rackSelector)).toHaveAttribute("aria-pressed", "true");

      const placed = await measureTrustedGesture(page, {
        triggerSelector: squareSelector,
        observedSelector: squareSelector,
        expected: { kind: "class", token: "tentative-square", present: true },
        input,
      });
      samples.placeInputToDomMs.push(placed.inputToDomMs);
      samples.placeInputToFrameOpportunityMs.push(placed.inputToFrameOpportunityMs);
      await expect(editor.locator(squareSelector)).toHaveClass(/tentative-square/);
    }

    await expect(editor.locator(".tentative-square")).toHaveCount(2);
    await expect(editor.locator(".committed-square")).toHaveCount(81);
    if (repetition === 0) {
      await testInfo.attach("trusted-" + input + "-populated-draft.png", {
        body: await page.screenshot(),
        contentType: "image/png",
      });
    }

    const reset = await measureTrustedGesture(page, {
      triggerSelector: ".draft-actions button:nth-child(2)",
      observedSelector: '[data-square-index="118"]',
      expected: { kind: "class", token: "tentative-square", present: false },
      input,
    });
    samples.resetInputToDomMs.push(reset.inputToDomMs);
    samples.resetInputToFrameOpportunityMs.push(reset.inputToFrameOpportunityMs);
    await expect(editor.locator(".tentative-square")).toHaveCount(0);
    await expect(editor.locator(".committed-square")).toHaveCount(81);
  }

  await recordEvidence(
    page,
    browser,
    testInfo,
    "trusted " + input + " pointer events; 81 committed tiles; 225 squares; 12 AH/reset cycles",
    samples,
    "trusted Playwright pointerdown event timestamp -> observed DOM state / second rAF; " +
      "excludes Playwright selector/actionability overhead; not hardware input or physical paint",
  );
  for (const [name, measurements] of Object.entries(samples)) {
    enforceBudget(summary(measurements).p95Ms, "Trusted " + input + ": " + name);
  }
}

test("50 ms p95: trusted mouse gestures update the populated draft", async ({
  page,
  browser,
}, testInfo) => {
  await verifyTrustedPopulatedGestures(page, browser, testInfo, "mouse");
});

test.describe("trusted touch input in phone-sized Chromium emulation", () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });

  test("50 ms p95: trusted touch gestures update the populated draft", async ({
    page,
    browser,
  }, testInfo) => {
    await verifyTrustedPopulatedGestures(page, browser, testInfo, "touch");
  });
});
