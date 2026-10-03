import { StateView, StateViewDescription } from "@moritzbrantner/ui/components/patterns/state-view";

export function App() {
  return (
    <main className="mx-auto max-w-xl p-6 text-foreground">
      <h1 className="text-2xl font-semibold">Scrabble</h1>
      <StateView variant="empty" className="mt-4">
        <StateViewDescription>
          Multiplayer play is being built. Game creation and joining are not available yet.
        </StateViewDescription>
      </StateView>
    </main>
  );
}
