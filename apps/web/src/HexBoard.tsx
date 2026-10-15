import { type CSSProperties, type ReactNode } from "react";
import { boardCells, hexBoardStyle, hexCellStyle } from "./board-geometry";

export function HexBoard({
  size,
  label,
  renderCell,
  cellLabel,
  className = "",
  style,
}: {
  size: number;
  label: string;
  renderCell: (row: number, column: number, premiumIndex: number) => ReactNode;
  cellLabel?: (row: number, column: number, premiumIndex: number) => string;
  className?: string;
  style?: CSSProperties;
}) {
  const cells = boardCells({ board_size: size, board_shape: "hexagon" });
  return (
    <table
      className={`hex-board ${className}`}
      aria-label={label}
      data-board-size={size}
      style={{ ...hexBoardStyle(size), ...style }}
    >
      <caption className="sr-only">{label}</caption>
      <tbody>
        {Array.from({ length: size }, (_, row) => (
          <tr key={row}>
            {cells
              .filter((cell) => cell.row === row)
              .map((cell) => (
                <td
                  key={cell.column}
                  style={hexCellStyle(size, row, cell.column)}
                  aria-label={cellLabel?.(row, cell.column, cell.premiumIndex)}
                >
                  {renderCell(row, cell.column, cell.premiumIndex)}
                </td>
              ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
