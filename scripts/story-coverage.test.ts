import { expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";

import { queryParameters, routeInventory } from "../apps/web/browser/stories/route-inventory";
import { stories, story } from "../apps/web/browser/stories/stories";
import { generatePaths } from "../apps/web/browser/stories/story-graph";
import { coverageListing } from "./story-coverage";

test("the committed coverage listing is current", () => {
  expect(readFileSync("docs/specs/story-coverage.md", "utf8")).toBe(coverageListing());
});

test("each path is non-empty, unique and reaches its model's every state", () => {
  for (const { id, model, events } of stories) {
    const paths = generatePaths(model, events, "pr");
    expect(new Set(paths.map((path) => path.name)).size).toBe(paths.length);
    const reached = new Set([
      String(model.getInitialSnapshot({} as never).value),
      ...paths.flatMap((path) => path.steps.map((step) => step.state)),
    ]);
    expect([...reached].sort(), id).toEqual(Object.keys(model.states).sort());
    // Every event the model declares is exercised by some path.
    expect(
      [...new Set(paths.flatMap((path) => path.steps.map((step) => step.event)))].sort(),
    ).toEqual([...events].sort());
  }
});

test("every inventory route names a real model state or says why it has none", () => {
  const states = Object.keys(story("ROUTE-NAV").model.states);
  for (const route of routeInventory) {
    if (route.state === undefined) {
      expect(route.notCovered, route.url).toBeTruthy();
    } else {
      expect(states, route.url).toContain(route.state);
    }
  }
  const listed = routeInventory.flatMap((route) => route.state ?? []);
  expect(states.filter((state) => !listed.includes(state))).toEqual([]);
});

test("every query parameter the application reads is inventoried", () => {
  const read = new Set<string>();
  for (const file of readdirSync("apps/web/src")) {
    if (!/\.tsx?$/.test(file) || /\.test\./.test(file)) {
      continue;
    }
    const source = readFileSync(`apps/web/src/${file}`, "utf8");
    if (!/URLSearchParams|searchParams/.test(source)) {
      continue;
    }
    for (const match of source.matchAll(
      /(?:searchParams|params|query|connection\??|URLSearchParams\([^)]*\))\.(?:get|getAll|has)\("([a-z-]+)"\)/g,
    )) {
      read.add(match[1] ?? "");
    }
    for (const match of source.matchAll(/for \(const (?:field|key) of \[([^\]]+)\]/g)) {
      for (const name of (match[1] ?? "").matchAll(/"([a-z-]+)"/g)) {
        read.add(name[1] ?? "");
      }
    }
  }
  expect(read.size).toBeGreaterThan(5);
  expect([...read].filter((name) => !(name in queryParameters))).toEqual([]);
});
