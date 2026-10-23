import { writeFile } from "node:fs/promises";
import { stripVTControlCharacters } from "node:util";

import { expect, test } from "@playwright/test";

import { states, type State } from "./state-graph/contract";
import {
  planJourneys,
  renderMarkdown,
  summarize,
  transition,
  type StepResult,
} from "./state-graph/plan";
import { openSession } from "./state-graph/session";

/**
 * Review scenario `state-graph` (#186): traverse every transition of the independent
 * expected-state contract. Each generated journey replays real UI actions in a fresh
 * disposable match, then the run writes a passed/failed/untested report with
 * per-view screenshots to test-results/review/.
 */
const expectedOf = new Map(states.map((state: State) => [state.id, state.expected]));

test("state-graph", async ({ browser, baseURL }, testInfo) => {
  test.setTimeout(15 * 60_000);
  const journeys = planJourneys();
  const steps: StepResult[] = [];
  for (const [index, journey] of journeys.entries()) {
    await test.step(`journey ${index + 1}: ${journey.join(" → ")}`, async () => {
      const session = await openSession(browser, baseURL ?? "");
      let blocked = false;
      try {
        for (const [position, id] of journey.entries()) {
          if (blocked) {
            steps.push({ journey: index, transition: id, status: "untested", screenshots: [] });
            continue;
          }
          const edge = transition(id);
          const step: StepResult = {
            journey: index,
            transition: id,
            status: "passed",
            screenshots: [],
          };
          await test.step(`${edge.from} —${id}→ ${edge.to}`, async () => {
            try {
              await session.actions[id]();
            } catch (error) {
              Object.assign(step, {
                status: "failed",
                failedAt: "action",
                error: stripVTControlCharacters(String(error)),
              });
            }
            const expected = expectedOf.get(edge.to);
            if (step.status === "passed" && expected !== undefined && expected !== null) {
              try {
                await session.observe(expected, edge.rejection);
              } catch (error) {
                Object.assign(step, {
                  status: "failed",
                  failedAt: "observe",
                  error: stripVTControlCharacters(String(error)),
                });
              }
            }
            // Each view is captured from its own browser context only.
            for (const [view, page] of Object.entries(session.views)) {
              const path = testInfo.outputPath(
                `journey-${index + 1}`,
                `${String(position + 1).padStart(2, "0")}-${id}-${view}.png`,
              );
              await page.screenshot({ path, fullPage: true }).then(
                () => step.screenshots.push(path),
                () => undefined,
              );
            }
          });
          steps.push(step);
          blocked = step.status === "failed";
        }
      } finally {
        await session.close();
      }
    });
  }

  const report = summarize(journeys, steps);
  const markdown = renderMarkdown(report);
  const json = testInfo.outputPath("state-graph-report.json");
  const md = testInfo.outputPath("state-graph-report.md");
  await writeFile(json, JSON.stringify(report, null, 2));
  await writeFile(md, markdown);
  await testInfo.attach("state-graph-report.md", { path: md, contentType: "text/markdown" });
  await testInfo.attach("state-graph-report.json", { path: json, contentType: "application/json" });
  console.log(markdown);
  console.log(`State-graph report: ${md}`);
  expect(
    report.transitions.filter((edge) => edge.status !== "passed").map((edge) => edge.id),
  ).toEqual([]);
});
