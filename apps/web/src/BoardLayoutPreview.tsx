import { HexBoard } from "./HexBoard";
import { useCopy } from "./preferences";
import { type ProfileCatalog } from "./profile-model";

type BoardProfile = ProfileCatalog["boardProfiles"][number];

const bonusKinds = ["double_letter", "triple_letter", "double_word", "triple_word"] as const;

export function BoardLayoutPreview({ board }: { board: BoardProfile }) {
  const { t, number } = useCopy();
  const indices = Array.from({ length: board.boardSize }, (_, index) => index);
  const center = Math.floor(board.boardSize / 2);

  return (
    <section className="board-layout-preview" role="group" aria-label={t("profiles.selectedBoard")}>
      <header className="board-layout-preview-header">
        <h2>{t("profiles.preview")}</h2>
        <p>
          {board.label} ·{" "}
          {t(board.boardShape === "hexagon" ? "hex.board" : "profiles.squareBoard", {
            size: number(board.boardSize),
          })}
        </p>
      </header>
      {board.boardShape === "hexagon" ? (
        <HexBoard
          size={board.boardSize}
          label={t("profiles.preview")}
          className="board-preview-grid"
          cellLabel={(row, column, index) =>
            t("board.square", {
              row: row + 1,
              column: column + 1,
              description: t(`premium.${board.premiums[index] ?? "normal"}`),
            })
          }
          renderCell={(row, column, index) => (
            <span
              className={`board-preview-cell premium-${(board.premiums[index] ?? "normal").replaceAll("_", "-")}`}
            >
              <span aria-hidden="true">
                {row === center && column === center
                  ? "★"
                  : t(`premiumShort.${board.premiums[index] ?? "normal"}`)}
              </span>
            </span>
          )}
        />
      ) : (
        <table
          className="board-preview-grid"
          aria-label={t("profiles.preview")}
          data-board-size={board.boardSize}
        >
          <caption className="sr-only">
            {t("profiles.squareBoard", { size: number(board.boardSize) })}
          </caption>
          <tbody>
            {indices.map((row) => (
              <tr key={row}>
                {indices.map((column) => {
                  const kind = board.premiums[row * board.boardSize + column] ?? "normal";
                  const isCenter = row === center && column === center;
                  const description = isCenter
                    ? `${t("profiles.center")} · ${t(`premium.${kind}`)}`
                    : t(`premium.${kind}`);
                  return (
                    <td
                      key={column}
                      aria-label={t("board.square", {
                        row: row + 1,
                        column: column + 1,
                        description,
                      })}
                    >
                      <span className={`board-preview-cell premium-${kind.replaceAll("_", "-")}`}>
                        <span aria-hidden="true">{isCenter ? "★" : t(`premiumShort.${kind}`)}</span>
                      </span>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <ul className="board-preview-legend" aria-label={t("profiles.bonusLegend")}>
        {bonusKinds.map((kind) => {
          const count = board.premiums.filter((cell) => cell === kind).length;
          return (
            <li key={kind}>
              <span className={`board-preview-swatch premium-${kind.replaceAll("_", "-")}`}>
                {t(`premiumShort.${kind}`)}
              </span>
              <span>
                {t(`premium.${kind}`)}: {number(count)}
              </span>
            </li>
          );
        })}
      </ul>
      <p className="setting-note">
        {board.premiumPolicy === "main_word_only"
          ? t("hex.mainPolicy")
          : t("profiles.premiumPolicy")}
      </p>
    </section>
  );
}
