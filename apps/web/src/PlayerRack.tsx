import { Button } from "@moritzbrantner/ui/client";
import { type PlayerSnapshot, type Ruleset } from "./public-state";

/** Receives only the authenticated recipient's rack; public scores contain no private tiles. */
export function PlayerRack({
  rack,
  rules,
  canAct,
  selection,
}: {
  rack: PlayerSnapshot["own_rack"];
  rules: Ruleset;
  canAct: boolean;
  selection?: {
    selected: string[];
    placed: string[];
    onSelect: (tileId: string) => void;
  };
}) {
  const values = new Map(
    rules.tiles.flatMap((tile) =>
      tile.face.kind === "letter" ? [[tile.face.letter, tile.value] as const] : [],
    ),
  );
  return (
    <section className="phone-rack" aria-label="Your tiles">
      <ul aria-label="Your rack" aria-disabled={!canAct}>
        {rack.tiles.map((tile) => {
          const blank = tile.face.kind === "blank";
          const letter = tile.face.kind === "letter" ? tile.face.letter : "◇";
          const value = blank ? 0 : values.get(letter);
          const description = `${blank ? "Blank tile" : letter}, ${value ?? "unknown"} points`;
          return (
            <li key={tile.id} data-tile-id={tile.id} aria-label={description}>
              {selection === undefined ? (
                <span className={`rack-tile${blank ? " blank-tile" : ""}`} aria-hidden="true">
                  <span>{letter}</span>
                  <small>{value ?? "?"}</small>
                </span>
              ) : (
                <Button
                  className={`rack-tile${blank ? " blank-tile" : ""}`}
                  data-placed={selection.placed.includes(tile.id)}
                  disabled={!canAct}
                  aria-label={`${description}${selection.placed.includes(tile.id) ? ", placed in draft" : ""}`}
                  aria-pressed={selection.selected.includes(tile.id)}
                  onClick={() => selection.onSelect(tile.id)}
                >
                  <span aria-hidden="true">{letter}</span>
                  <small aria-hidden="true">{value ?? "?"}</small>
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      {rack.tiles.length === 0 && <p>Your rack is empty.</p>}
    </section>
  );
}
