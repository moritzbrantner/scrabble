import { expect, test } from "bun:test";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { z } from "zod";

import data from "./fixtures/private-rack.json";
import { PlayerRack } from "./PlayerRack";
import { id, playerSnapshot, ruleset } from "./public-state";

const fixtures = z
  .strictObject({
    ruleset,
    snapshots: z.strictObject({
      playing: playerSnapshot,
      committed: playerSnapshot,
      drawn: playerSnapshot,
    }),
    otherTileIds: z.array(id),
  })
  .parse(data);
function render(snapshot: z.infer<typeof playerSnapshot>, canAct = true) {
  return renderToStaticMarkup(
    createElement(PlayerRack, { rack: snapshot.own_rack, rules: fixtures.ruleset, canAct }),
  );
}

test("private tiles show authoritative letters, values and blanks without another rack", () => {
  const html = render(fixtures.snapshots.playing);
  expect(html).toContain('aria-label="Blank tile, 0 points"');
  for (const tile of fixtures.snapshots.playing.own_rack.tiles) {
    expect(html).toContain(`data-tile-id="${tile.id}"`);
    if (tile.face.kind === "letter") {
      const face = tile.face;
      const definition = fixtures.ruleset.tiles.find(
        (entry) => entry.face.kind === "letter" && entry.face.letter === face.letter,
      );
      expect(html).toContain(`aria-label="${face.letter}, ${definition?.value} points"`);
    }
  }
  for (const other of fixtures.otherTileIds) {
    expect(html).not.toContain(`data-tile-id="${other}"`);
  }
  expect(html).not.toContain("input");
  expect(html).not.toContain("button");
  expect(render(fixtures.snapshots.playing, false)).toContain('aria-disabled="true"');
});

test("authoritative commit and draw projections replace rack contents without local mutation", () => {
  const original = render(fixtures.snapshots.playing);
  const committed = render(fixtures.snapshots.committed);
  const drawn = render(fixtures.snapshots.drawn);
  expect(fixtures.snapshots.playing.own_rack.tiles).toHaveLength(7);
  expect(fixtures.snapshots.committed.own_rack.tiles).toHaveLength(5);
  expect(fixtures.snapshots.drawn.own_rack.tiles).toHaveLength(7);
  const committedIds = new Set(fixtures.snapshots.committed.own_rack.tiles.map((tile) => tile.id));
  for (const tile of fixtures.snapshots.playing.own_rack.tiles) {
    if (!committedIds.has(tile.id)) {
      expect(committed).not.toContain(`data-tile-id="${tile.id}"`);
      expect(drawn).not.toContain(`data-tile-id="${tile.id}"`);
    }
  }
  const newTiles = fixtures.snapshots.drawn.own_rack.tiles.filter(
    (tile) => !committedIds.has(tile.id),
  );
  expect(newTiles).toHaveLength(2);
  for (const tile of newTiles) {
    expect(original).not.toContain(`data-tile-id="${tile.id}"`);
    expect(drawn).toContain(`data-tile-id="${tile.id}"`);
  }
  expect(render(fixtures.snapshots.playing)).toBe(original);
});
