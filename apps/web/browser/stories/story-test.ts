// Runs one story's generated paths as Playwright tests. Each path gets fresh browser state.
import { test, type APIRequestContext, type Browser, type Page } from "../fixtures";
import { story } from "./stories";
import { generatePaths, type StoryPath, type Tier } from "./story-graph";

export const storyTier: Tier = process.env.SCRABBLE_STORY_TIER === "full" ? "full" : "pr";

export type StoryFixtures = { page: Page; browser: Browser; request: APIRequestContext };

export type Driver<C> = {
  /** Fresh state for one path; what `close` releases must not outlive it. */
  open: (fixtures: StoryFixtures) => Promise<C>;
  close?: (world: C) => Promise<void>;
  /** The real user action for each model event. */
  events: Record<string, (world: C) => Promise<void>>;
  /** What must be observably true in each model state, checked after every step. */
  states: Record<string, (world: C) => Promise<void>>;
  /** Checked once before the first event. */
  initial: (world: C) => Promise<void>;
  /** Paths that hit a filed bug: `test.fixme` with the issue link. Never encode the bug. */
  knownBugs?: readonly { path: RegExp; issue: string }[];
};

export function storyTests<C>(id: string, driver: Driver<C>) {
  const definition = story(id);
  test.describe(`${id}: ${definition.title}`, () => {
    for (const path of generatePaths(definition.model, definition.events, storyTier)) {
      definePath(id, path, driver);
    }
  });
}

function definePath<C>(id: string, path: StoryPath, driver: Driver<C>) {
  const details = { tag: ["@story", `@story:${id}`] };
  const bug = driver.knownBugs?.find((candidate) => candidate.path.test(path.name));
  if (bug !== undefined) {
    test.fixme(
      path.name,
      { ...details, annotation: { type: "issue", description: bug.issue } },
      () => {},
    );
    return;
  }
  test(path.name, details, async ({ page, browser, request }) => {
    test.setTimeout(30_000 + path.steps.length * 5_000);
    const world = await driver.open({ page, browser, request });
    try {
      await driver.initial(world);
      for (const step of path.steps) {
        await test.step(`${step.event} -> ${step.state}`, async () => {
          const event = driver.events[step.event];
          const state = driver.states[step.state];
          if (event === undefined || state === undefined) {
            throw new Error(`${id} has no driver for ${step.event} -> ${step.state}`);
          }
          await event(world);
          await state(world);
        });
      }
    } finally {
      await driver.close?.(world);
    }
  });
}
