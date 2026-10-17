// A recipient's view of a shared custom word list (#97; Decisions 1.16, 1.18).
// The link grants use and copy only; no owner capability is ever involved.
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useState } from "react";

import { type Copy } from "./copy";
import { wordCount, wordListFailure } from "./CustomWordLists";
import { useCopy } from "./preferences";
import {
  copySharedWordList,
  fetchSharedWordList,
  type SharedWordList,
  type WordChoice,
} from "./word-list-library";

export function SharedWordListView({
  api,
  shareId,
  onUse,
}: {
  api: string;
  shareId: string;
  onUse?: (choice: WordChoice) => void;
}) {
  const { t } = useCopy();
  const [shared, setShared] = useState<SharedWordList>();
  const [unavailable, setUnavailable] = useState(false);
  const [copy, setCopy] = useState<
    { kind: "idle" | "saving" | "saved" } | { kind: "failed"; message: Copy }
  >({ kind: "idle" });
  useEffect(() => {
    let active = true;
    void fetchSharedWordList(api, shareId)
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
  async function saveCopy() {
    if (!shared || copy.kind === "saving") {
      return;
    }
    setCopy({ kind: "saving" });
    try {
      await copySharedWordList(api, shared.shareId, shared.name);
      setCopy({ kind: "saved" });
    } catch (error) {
      setCopy({ kind: "failed", message: wordListFailure(error) });
    }
  }
  return (
    <main className="create-board">
      <h1>Scrabble</h1>
      <section className="shared-layout" aria-label={t("sharedWords.region")}>
        {!shared && !unavailable && <p role="status">{t("sharedWords.loading")}</p>}
        {unavailable && <p role="alert">{t("sharedWords.unavailable")}</p>}
        {shared && (
          <>
            {/* Separate text nodes keep the name and count distinct words for all readers. */}
            <h2>{shared.name}</h2> <p>{t(wordCount(shared.wordCount))}</p>{" "}
            <p className="setting-note">{t("sharedWords.note")}</p>
            <div className="layout-editor-actions">
              <Button
                type="button"
                disabled={copy.kind === "saving"}
                onClick={() => void saveCopy()}
              >
                {t("shared.copy")}
              </Button>
              {onUse && (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() =>
                    onUse({
                      value: `word-share:${shared.shareId}`,
                      label: shared.name,
                      identity: shared.wordList,
                      access: { shareId: shared.shareId },
                    })
                  }
                >
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
