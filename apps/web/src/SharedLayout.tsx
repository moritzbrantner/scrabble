// A recipient's view of a shared custom board layout (#101; Decision 1.33).
// The link grants use and copy only; no owner capability is ever involved.
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useState } from "react";

import { BoardLayoutPreview } from "./BoardLayoutPreview";
import { type Copy } from "./copy";
import { type BoardChoice, layoutFailure } from "./CustomLayouts";
import { boardRow, copyShared, fetchShared, type SharedLayout } from "./layout-library";
import { useCopy } from "./preferences";

export function SharedLayoutView({
  api,
  shareId,
  onUse,
}: {
  api: string;
  shareId: string;
  onUse?: (choice: BoardChoice) => void;
}) {
  const { t } = useCopy();
  const [shared, setShared] = useState<SharedLayout>();
  const [unavailable, setUnavailable] = useState(false);
  const [copy, setCopy] = useState<
    { kind: "idle" | "saving" | "saved" } | { kind: "failed"; message: Copy }
  >({ kind: "idle" });
  useEffect(() => {
    let active = true;
    void fetchShared(api, shareId)
      .then((value) => {
        if (active) {
          setShared(value);
        }
      })
      .catch(() => {
        if (active) {
          setUnavailable(true);
        }
      });
    return () => {
      active = false;
    };
  }, [api, shareId]);
  const choice: BoardChoice | undefined = shared && {
    value: `share:${shared.shareId}`,
    row: boardRow(shared.name, shared.board, shared.layout),
  };
  async function saveCopy() {
    if (!shared || copy.kind === "saving") {
      return;
    }
    setCopy({ kind: "saving" });
    try {
      await copyShared(api, shared.shareId, shared.name);
      setCopy({ kind: "saved" });
    } catch (error) {
      setCopy({ kind: "failed", message: layoutFailure(error) });
    }
  }
  return (
    <main className="create-board">
      <h1>Scrabble</h1>
      <section className="shared-layout" aria-label={t("shared.region")}>
        {!shared && !unavailable && <p role="status">{t("shared.loading")}</p>}
        {unavailable && <p role="alert">{t("shared.unavailable")}</p>}
        {shared && choice && (
          <>
            <h2>{shared.name}</h2>
            <p className="setting-note">{t("shared.note")}</p>
            <BoardLayoutPreview board={choice.row} />
            <div className="layout-editor-actions">
              <Button
                type="button"
                disabled={copy.kind === "saving"}
                onClick={() => void saveCopy()}
              >
                {t("shared.copy")}
              </Button>
              {onUse && (
                <Button type="button" variant="outline" onClick={() => onUse(choice)}>
                  {t("shared.use")}
                </Button>
              )}
            </div>
            <p role="status">{copy.kind === "saved" ? t("shared.copied") : ""}</p>
            {copy.kind === "failed" && <p role="alert">{t(copy.message)}</p>}
          </>
        )}
      </section>
    </main>
  );
}
