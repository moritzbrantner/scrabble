import {
  exclusions,
  initialState,
  states,
  transitions,
  type StateId,
  type Transition,
  type TransitionId,
} from "./contract";

/** One fresh disposable match: transitions replayed from the initial state. */
export type Journey = readonly TransitionId[];

const byId = new Map(transitions.map((transition) => [transition.id, transition]));

export function transition(id: TransitionId): Transition {
  const found = byId.get(id);
  if (found === undefined) {
    throw new Error(`Unknown transition ${id}`);
  }
  return found;
}

/** Shortest route from `from` whose last edge is still untested (declaration order breaks ties). */
function routeToUntested(from: StateId, untested: ReadonlySet<TransitionId>) {
  const routes = new Map<StateId, TransitionId[]>([[from, []]]);
  const queue: StateId[] = [from];
  for (let state = queue.shift(); state !== undefined; state = queue.shift()) {
    const route = routes.get(state) ?? [];
    const outgoing = transitions.filter((edge) => edge.from === state);
    const target = outgoing.find((edge) => untested.has(edge.id));
    if (target !== undefined) {
      return [...route, target.id];
    }
    for (const edge of outgoing) {
      if (!routes.has(edge.to)) {
        routes.set(edge.to, [...route, edge.id]);
        queue.push(edge.to);
      }
    }
  }
  return undefined;
}

/**
 * Generate journeys that execute every declared transition at least once. Each journey
 * starts a fresh match and keeps extending while an untested edge is reachable.
 */
export function planJourneys(): Journey[] {
  const untested = new Set(transitions.map((edge) => edge.id));
  const journeys: Journey[] = [];
  while (untested.size > 0) {
    const journey: TransitionId[] = [];
    let state = initialState;
    for (
      let route = routeToUntested(state, untested);
      route !== undefined;
      route = routeToUntested(state, untested)
    ) {
      for (const id of route) {
        untested.delete(id);
        state = transition(id).to;
      }
      journey.push(...route);
    }
    if (journey.length === 0) {
      throw new Error(`Unreachable transitions: ${[...untested].join(", ")}`);
    }
    journeys.push(journey);
  }
  return journeys;
}

export type Status = "passed" | "failed" | "untested";

/** One executed (or skipped) step of a journey. */
export type StepResult = {
  journey: number;
  transition: TransitionId;
  status: Status;
  /** Failure while performing the action, or while checking the reached state. */
  failedAt?: "action" | "observe";
  error?: string;
  screenshots: string[];
};

export type Report = {
  states: { id: StateId; label: string; status: Status }[];
  transitions: {
    id: TransitionId;
    from: StateId;
    to: StateId;
    kind: Transition["kind"];
    action: string;
    status: Status;
    /** Reproducible action path from the initial state through this edge. */
    path: TransitionId[];
    error?: string;
    screenshots: string[];
  }[];
  journeys: { transitions: Journey; status: Status }[];
};

function combine(statuses: Status[]): Status {
  if (statuses.includes("failed")) {
    return "failed";
  }
  return statuses.includes("passed") ? "passed" : "untested";
}

export function summarize(journeys: readonly Journey[], steps: readonly StepResult[]): Report {
  const stateStatuses = new Map<StateId, Status[]>();
  const record = (state: StateId, status: Status) =>
    stateStatuses.set(state, [...(stateStatuses.get(state) ?? []), status]);
  for (const step of steps) {
    const edge = transition(step.transition);
    if (step.status === "passed") {
      record(edge.to, "passed");
    } else if (step.status === "failed" && step.failedAt === "observe") {
      record(edge.to, "failed");
    }
  }
  // The initial page is reached whenever a journey performs its first action.
  if (
    steps.some(
      (step) => step.status !== "untested" && transition(step.transition).from === initialState,
    )
  ) {
    record(initialState, "passed");
  }
  return {
    states: states.map(({ id, label }) => ({
      id,
      label,
      status: combine(stateStatuses.get(id) ?? []),
    })),
    transitions: transitions.map((edge) => {
      const own = steps.filter((step) => step.transition === edge.id);
      const status = combine(own.map((step) => step.status));
      const evidence = own.find((step) => step.status === status) ?? own[0];
      const journey = evidence === undefined ? undefined : journeys[evidence.journey];
      const index = evidence === undefined ? -1 : steps.indexOf(evidence);
      const position = steps
        .slice(0, index + 1)
        .filter((step) => step.journey === evidence?.journey).length;
      return {
        id: edge.id,
        from: edge.from,
        to: edge.to,
        kind: edge.kind,
        action: edge.action,
        status,
        path: journey?.slice(0, position) ?? [],
        ...(evidence?.error === undefined ? {} : { error: evidence.error }),
        screenshots: evidence?.screenshots ?? [],
      };
    }),
    journeys: journeys.map((journey, index) => ({
      transitions: journey,
      status: combine(steps.filter((step) => step.journey === index).map((step) => step.status)),
    })),
  };
}

export function renderMarkdown(report: Report): string {
  const lines = [
    "# Scrabble state-graph exploration",
    "",
    "Coverage of the declared contract in `apps/web/review/state-graph/contract.ts` only;",
    "it does not prove complete application correctness.",
    "",
    "## States",
    "",
    "| State | Status | Label |",
    "| --- | --- | --- |",
    ...report.states.map((state) => `| ${state.id} | ${state.status} | ${state.label} |`),
    "",
    "## Transitions",
    "",
    "| Transition | Kind | Edge | Status | Action path | Notes |",
    "| --- | --- | --- | --- | --- | --- |",
    ...report.transitions.map(
      (edge) =>
        `| ${edge.id} | ${edge.kind} | ${edge.from} → ${edge.to} | ${edge.status} | ${edge.path.join(" → ")} | ${(edge.error ?? "").replaceAll("|", "\\|").replaceAll("\n", " ").slice(0, 300)} |`,
    ),
    "",
    "## Journeys (each in a fresh disposable match)",
    "",
    ...report.journeys.map(
      (journey, index) => `${index + 1}. ${journey.status}: ${journey.transitions.join(" → ")}`,
    ),
    "",
    "## Intentional exclusions",
    "",
    ...exclusions.map((exclusion) => `- ${exclusion.area}: ${exclusion.reason}`),
    "",
  ];
  return lines.join("\n");
}
