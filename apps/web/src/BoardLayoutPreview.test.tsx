import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import { BoardLayoutPreview } from "./BoardLayoutPreview";
import { type ProfileCatalog } from "./profile-model";

type BoardProfile = ProfileCatalog["boardProfiles"][number];

test("a board preview renders the exact configured geometry and bonus squares", () => {
  const board: BoardProfile = {
    identity: { name: "compact", revision: "1" },
    label: "Compact square",
    boardSize: 7,
    premiums: Array.from({ length: 49 }, () => "normal"),
  };
  board.premiums[0] = "triple_word";
  board.premiums[24] = "double_word";
  const compact = renderToStaticMarkup(<BoardLayoutPreview board={board} />);
  expect((compact.match(/<td /g) ?? []).length).toBe(49);
  expect(compact).toContain('data-board-size="7"');
  expect(compact).toContain("Triple word");
  expect(compact).toContain("Starting square");
  expect(compact).toContain("★");

  const large = renderToStaticMarkup(
    <BoardLayoutPreview
      board={{
        ...board,
        boardSize: 15,
        premiums: Array.from({ length: 225 }, () => "normal"),
      }}
    />,
  );
  expect((large.match(/<td /g) ?? []).length).toBe(225);
  expect(large).toContain('data-board-size="15"');
});
