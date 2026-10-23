import { describe, expect, test } from "bun:test";

import { initialState, states, transitions } from "./contract";
import { planJourneys, renderMarkdown, summarize, transition, type StepResult } from "./plan";

describe("state-graph contract", () => {
  test("declares unique ids and edges between declared states", () => {
    const stateIds = states.map((state) => state.id);
    expect(new Set(stateIds).size).toBe(stateIds.length);
    expect(new Set(transitions.map((edge) => edge.id)).size).toBe(transitions.length);
    for (const edge of transitions) {
      expect(stateIds).toContain(edge.from);
      expect(stateIds).toContain(edge.to);
      expect(edge.kind === "negative").toBe(edge.rejection !== undefined);
    }
  });

  test("journeys start at the initial state, follow edges and cover every transition", () => {
    const journeys = planJourneys();
    const covered = new Set(journeys.flat());
    expect([...covered].sort()).toEqual(transitions.map((edge) => edge.id).sort());
    const reached = new Set([initialState]);
    for (const journey of journeys) {
      let state = initialState;
      for (const id of journey) {
        expect(transition(id).from).toBe(state);
        state = transition(id).to;
        reached.add(state);
      }
    }
    expect([...reached].sort()).toEqual(states.map((state) => state.id).sort());
  });

  test("reports failures, untested edges and reproducible action paths", () => {
    const journeys = planJourneys();
    const first = journeys[0] ?? [];
    const steps: StepResult[] = first.map((id, index) => ({
      journey: 0,
      transition: id,
      status: index < 2 ? "passed" : "untested",
      screenshots: [],
    }));
    steps[2] = { ...steps[2]!, status: "failed", failedAt: "observe", error: "boom" };
    const report = summarize(journeys, steps);
    const failed = report.transitions.find((edge) => edge.id === first[2]);
    expect(failed?.status).toBe("failed");
    expect(failed?.path).toEqual(first.slice(0, 3));
    expect(report.states.find((state) => state.id === failed?.to)?.status).toBe("failed");
    expect(report.states.find((state) => state.id === initialState)?.status).toBe("passed");
    expect(report.transitions.find((edge) => edge.id === "start-rematch")?.status).toBe("untested");
    expect(report.journeys[0]?.status).toBe("failed");
    expect(renderMarkdown(report)).toContain("does not prove complete application correctness");
  });
});
