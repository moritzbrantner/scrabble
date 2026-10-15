import { type MoveAnalysis as Analysis } from "./public-state";
import { useCopy } from "./preferences";

export function MoveAnalysis({
  analysis,
  preview = false,
}: {
  analysis: Analysis;
  preview?: boolean;
}) {
  const { t, number } = useCopy();
  const content = (
    <>
      <p>{t("hex.main", { word: analysis.words[0]?.text ?? "" })} </p>
      <p>{t(analysis.length_metric === "tiles" ? "hex.tiles" : "hex.letters")} </p>
      <p>{t("hex.tie")} </p>
      <ul>
        {analysis.words.map((word, index) => (
          <li key={index}>
            {word.text} · {t(`hex.${word.direction}`)} · {number(word.score)}{" "}
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
