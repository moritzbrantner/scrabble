import { faceText, type PlayerSnapshot } from "./public-state";
import { type MoveDraft } from "./move-draft";

/** Never apply a delayed evaluation to another draft, player, turn or connection. */
export function draftEvaluation(snapshot: PlayerSnapshot, draft: MoveDraft) {
  const preview = snapshot.public.preview;
  if (
    snapshot.public.configuration.board_shape !== "hexagon" ||
    snapshot.public.phase.kind !== "playing" ||
    preview?.player_id !== snapshot.own_rack.player_id ||
    preview.turn !== snapshot.public.phase.turn ||
    draft.placements.length === 0
  ) {
    return undefined;
  }
  const expected = draft.placements.map((placement) => {
    const tile = snapshot.own_rack.tiles.find((tile) => tile.id === placement.tile_id);
    return {
      coordinate: placement.coordinate,
      letter: placement.blank_as ?? (tile && faceText(tile.face)),
      is_blank: tile?.face.kind === "blank",
    };
  });
  const normalize = (tiles: typeof expected) =>
    JSON.stringify(
      [...tiles]
        .sort(
          (a, b) =>
            a.coordinate.row - b.coordinate.row || a.coordinate.column - b.coordinate.column,
        )
        .map((tile) => [tile.coordinate.row, tile.coordinate.column, tile.letter, tile.is_blank]),
    );
  return normalize(expected) === normalize(preview.tiles) ? preview.evaluation : undefined;
}
