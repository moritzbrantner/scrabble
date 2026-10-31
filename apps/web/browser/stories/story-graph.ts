// Path generation for the story suites (docs/specs/story-e2e.md). No browser imports, so
// `bun test` and the coverage listing can use it too.
import type { AnyMachineSnapshot, AnyStateMachine } from "xstate";
import { getAdjacencyMap, getShortestPaths, getSimplePaths } from "xstate/graph";

export type Tier = "pr" | "full";
export type StoryStep = { event: string; state: string };
export type StoryPath = { name: string; steps: readonly StoryStep[] };

/** A hard ceiling for each model, so a model change cannot silently explode the suite. */
const MAX_PATHS: Record<Tier, number> = { pr: 60, full: 400 };

type Snapshot = { value: unknown };
type RawPath = { steps: readonly { event: { type: string }; state: Snapshot }[] };

function steps(path: RawPath): StoryStep[] {
  return path.steps
    .filter((step) => step.event.type !== "xstate.init")
    .map((step) => ({ event: step.event.type, state: String(step.state.value) }));
}

const named = (path: readonly StoryStep[]): StoryPath => ({
  name: path.map((step) => step.event).join(" > "),
  steps: path,
});

/** Every step is asserted, so a path that is a prefix of a longer one adds no evidence. */
function withoutPrefixes(paths: StoryStep[][]) {
  const unique = new Map(
    paths
      .filter((path) => path.length > 0)
      .map((path) => [`${path.map((step) => step.event).join(" ")} `, path]),
  );
  return [...unique]
    .filter(([key]) => ![...unique.keys()].some((other) => other !== key && other.startsWith(key)))
    .map(([, path]) => path);
}

/**
 * `pr`: a shortest path to every state, extended by one step for each transition those paths
 * miss. Shortest paths alone only reach states: they never take a self-loop such as a reload or
 * a rejected action, which are the transitions these stories exist to check.
 * `full`: the `pr` paths plus every simple path, bounded by MAX_PATHS.
 */
export function generatePaths(
  model: AnyStateMachine,
  events: readonly string[],
  tier: Tier,
): StoryPath[] {
  const options = {
    events: events.map((type) => ({ type })),
    // Without this, the traversal treats every unhandled event as a transition to the same state.
    filterEvents: (snapshot: AnyMachineSnapshot, event: { type: string }) => snapshot.can(event),
  };
  const generated: StoryStep[][] = [];
  if (tier === "full") {
    // Simple paths never repeat a state, so they add routes but never take self-loops.
    generated.push(...(getSimplePaths(model, options) as unknown as RawPath[]).map(steps));
  }
  {
    const shortest = new Map<string, StoryStep[]>();
    for (const path of getShortestPaths(model, options) as unknown as (RawPath & {
      state: Snapshot;
    })[]) {
      shortest.set(JSON.stringify(path.state), steps(path));
    }
    generated.push(...shortest.values());
    const adjacency = getAdjacencyMap(model, options) as unknown as Record<
      string,
      { state: Snapshot; transitions: Record<string, { event: { type: string }; state: Snapshot }> }
    >;
    // Edge identity is its source path, as each vertex is extended from its one shortest path.
    const taken = new Set<string>();
    const edge = (from: StoryStep[], event: string) =>
      `${from.map((step) => step.event).join(" ")}|${event}`;
    for (const path of generated) {
      for (const [index, step] of path.entries()) {
        taken.add(edge(path.slice(0, index), step.event));
      }
    }
    for (const vertex of Object.values(adjacency)) {
      const source = shortest.get(JSON.stringify(vertex.state));
      if (source === undefined) {
        throw new Error(`No shortest path reaches ${JSON.stringify(vertex.state)}`);
      }
      for (const transition of Object.values(vertex.transitions)) {
        if (!taken.has(edge(source, transition.event.type))) {
          generated.push([
            ...source,
            { event: transition.event.type, state: String(transition.state.value) },
          ]);
        }
      }
    }
  }
  const paths = withoutPrefixes(generated)
    .map(named)
    .sort((left, right) => left.name.localeCompare(right.name));
  if (paths.length > MAX_PATHS[tier]) {
    throw new Error(`${paths.length} ${tier} paths exceed the bound of ${MAX_PATHS[tier]}`);
  }
  return paths;
}
