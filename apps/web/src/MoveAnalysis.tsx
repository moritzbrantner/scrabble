import { type CopyKey } from "./copy";
import { type MoveAnalysis as Analysis } from "./public-state";
import { useCopy } from "./preferences";

export function MoveAnalysis({
  analysis,
  preview = false,
  hex = true,
  note,
}: {
  analysis: Analysis;
  preview?: boolean;
  hex?: boolean;
  /** Labels an unsubmitted score as an estimate and says what was (not) checked. */
  note?: Extract<CopyKey, `estimate.${string}`>;
}) {
  const { t, number } = useCopy();
  const content = (
    <>
      {note !== undefined && (
        <p>
          <strong>{t("estimate.title", { points: analysis.total })}</strong> {t(note)}
        </p>
      )}
      <p>{t("hex.main", { word: analysis.words[0]?.text ?? "" })} </p>
      {hex && <p>{t(analysis.length_metric === "tiles" ? "hex.tiles" : "hex.letters")} </p>}
      {hex && <p>{t("hex.tie")} </p>}
      <ul>
        {analysis.words.map((word, index) => (
          <li key={index}>
            {word.text} · {t(hex ? `hex.${word.direction}` : `estimate.${word.direction}`)} ·{" "}
            {number(word.score)}{" "}
          </li>
        ))}
      </ul>
      {analysis.bingo_bonus > 0 && <p>{t("hex.bingo", { points: analysis.bingo_bonus })} </p>}
      <p>{t("hex.total", { points: analysis.total })} </p>
    </>
  );
  return preview ? (
    <section className="move-analysis" aria-label={t("hex.preview")}>
      {content}
    </section>
  ) : (
    <div className="move-analysis">{content}</div>
  );
}
