/** Browser-test harness only; absent from the application's production import graph. */
import { createRoot, type Root } from "react-dom/client";
import { MoveEditor } from "../src/MoveEditor";
import { type PlayerSnapshot, type Ruleset } from "../src/public-state";
let root: Root | undefined;
export function showEditor(snapshot: PlayerSnapshot, rules: Ruleset) {
  if (root === undefined) {
    const app = document.getElementById("root");
    if (app !== null) {
      app.hidden = true;
    }
    const container = document.createElement("main");
    container.className = "player-phone";
    document.body.append(container);
    root = createRoot(container);
  }
  root.render(
    <MoveEditor
      snapshot={snapshot}
      rules={rules}
      canAct={
        snapshot.public.phase.kind === "playing" &&
        snapshot.public.phase.active_player === snapshot.own_rack.player_id
      }
      onTurnAction={async () => {
        throw new Error("No transport in the isolated editor fixture");
      }}
    />,
  );
}
