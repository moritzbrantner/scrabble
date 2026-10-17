// Guest-owned custom word-list library and editor (#97; Decisions 1.8, 1.16, 1.20, 1.45).
// The server is the only authority on the word-list format; this view imports a text
// file, lets the player review it, and reports server feedback precisely.
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useId, useRef, useState, type ChangeEvent } from "react";

import { type Copy, type CopyKey } from "./copy";
import { CopyLinkButton } from "./CustomLayouts";
import { useCopy } from "./preferences";
import {
  createWordList,
  deleteWordList,
  fetchWordListLimits,
  forgetWordList,
  importWordListFile,
  ownedWordLists,
  readWordList,
  shareWordList,
  unshareWordList,
  updateWordList,
  WordListError,
  wordListShareUrl,
  type OwnedWordList,
  type WordChoice,
  type WordListLimits,
  type WordListOwner,
} from "./word-list-library";

type Owned = { entry: WordListOwner; saved: OwnedWordList };

export function wordCount(count: number): Copy {
  return count === 1
    ? { key: "wordLists.countOne" }
    : { key: "wordLists.count", values: { count } };
}
function sizeValues(limits: WordListLimits | undefined) {
  // Until the server's bounds load, show its documented defaults; the server decides.
  return { bytes: limits?.maxBytes ?? 1_048_576, words: limits?.maxWords ?? 200_000 };
}
export function wordListFailure(error: unknown, limits?: WordListLimits): Copy {
  if (error instanceof WordListError) {
    if (error.code === "invalid") {
      if (error.line !== undefined) {
        return { key: "wordEditor.invalidLine", values: { line: error.line } };
      }
      return { key: "wordEditor.rejected", values: { reason: error.reason ?? "" } };
    }
    if (error.code === "tooLarge") {
      return limits
        ? { key: "wordEditor.tooLarge", values: sizeValues(limits) }
        : { key: "wordEditor.rejected", values: { reason: error.reason ?? "" } };
    }
    if (error.code === "full") {
      return { key: "wordLists.full" };
    }
    if (error.code === "storage") {
      return { key: "wordLists.storage" };
    }
  }
  return { key: "wordLists.failed" };
}

/** The create page's word-list library plus its editor; reports saved lists as choices. */
export function CustomWordLists({
  api,
  onChoices,
}: {
  api: string;
  onChoices: (choices: WordChoice[]) => void;
}) {
  const { t } = useCopy();
  const [owned, setOwned] = useState<Owned[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [limits, setLimits] = useState<WordListLimits>();
  const [editing, setEditing] = useState<{ key: number; wordListId?: string }>();
  const [shared, setShared] = useState<{ wordListId: string; url: string }>();
  const [message, setMessage] = useState<{ text: CopyKey; alert: boolean }>();
  const editorKey = useRef(0);

  useEffect(() => {
    let active = true;
    void fetchWordListLimits(api)
      .then((value) => {
        if (active) {
          setLimits(value);
        }
      })
      .catch(() => undefined);
    const entries = ownedWordLists(api);
    void Promise.all(
      entries.map(async (entry) => {
        try {
          return { entry, saved: await readWordList(api, entry) };
        } catch (error) {
          if (error instanceof WordListError && error.code === "unknown") {
            // Only the issuing API is ever asked, so its answer is authoritative.
            forgetWordList(entry);
            return undefined;
          }
          if (active) {
            setLoadFailed(true);
          }
          return undefined;
        }
      }),
    ).then((rows) => {
      if (active) {
        setOwned(rows.filter((row): row is Owned => row !== undefined));
        setLoading(false);
      }
    });
    return () => {
      active = false;
    };
  }, [api]);
  useEffect(() => {
    onChoices(
      owned.map(({ entry, saved }) => ({
        value: `word-list:${saved.wordListId}`,
        label: saved.name,
        identity: saved.wordList,
        access: { wordListId: entry.wordListId, ownerCapability: entry.ownerCapability },
      })),
    );
  }, [owned, onChoices]);

  function open(wordListId?: string) {
    editorKey.current += 1;
    setMessage(undefined);
    setEditing({ key: editorKey.current, ...(wordListId === undefined ? {} : { wordListId }) });
  }
  function replace(saved: OwnedWordList) {
    setOwned((rows) =>
      rows.map((row) => (row.saved.wordListId === saved.wordListId ? { ...row, saved } : row)),
    );
  }
  async function save(wordListId: string | undefined, name: string, words: string) {
    const current = owned.find((row) => row.saved.wordListId === wordListId);
    if (current) {
      replace(await updateWordList(api, current.entry, name, words));
      return;
    }
    const created = await createWordList(api, name, words);
    const entry = ownedWordLists(api).find((row) => row.wordListId === created.wordListId);
    if (entry) {
      const saved = await readWordList(api, entry).catch(() => ({ ...created, words }));
      setOwned((rows) => [...rows, { entry, saved }]);
    }
    setEditing((value) => (value ? { ...value, wordListId: created.wordListId } : value));
  }
  async function remove(row: Owned) {
    setMessage(undefined);
    try {
      await deleteWordList(api, row.entry);
    } catch (error) {
      if (!(error instanceof WordListError && error.code === "unknown")) {
        setMessage({ text: "wordLists.failed", alert: true });
        return;
      }
      forgetWordList(row.entry);
    }
    const id = row.saved.wordListId;
    setOwned((rows) => rows.filter((item) => item.saved.wordListId !== id));
    setEditing((value) => (value?.wordListId === id ? undefined : value));
    setShared((value) => (value?.wordListId === id ? undefined : value));
    setMessage({ text: "wordLists.deleted", alert: false });
  }
  async function share(row: Owned) {
    setMessage(undefined);
    try {
      const id = row.saved.shareId ?? (await shareWordList(api, row.entry));
      replace({ ...row.saved, shareId: id });
      setShared({ wordListId: row.saved.wordListId, url: wordListShareUrl(id) });
    } catch {
      setMessage({ text: "wordLists.failed", alert: true });
    }
  }
  async function unshare(row: Owned) {
    setMessage(undefined);
    try {
      await unshareWordList(api, row.entry);
      const withdrawn = { ...row.saved };
      delete withdrawn.shareId;
      replace(withdrawn);
      setShared((value) => (value?.wordListId === row.saved.wordListId ? undefined : value));
      setMessage({ text: "wordLists.unshared", alert: false });
    } catch {
      setMessage({ text: "wordLists.failed", alert: true });
    }
  }
  const editingRow = owned.find((row) => row.saved.wordListId === editing?.wordListId);
  const sharedName = owned.find((row) => row.saved.wordListId === shared?.wordListId)?.saved.name;

  return (
    <>
      <section className="layout-library" aria-label={t("wordLists.region")}>
        <h2>{t("wordLists.region")}</h2>
        <p role="note" className="setting-note">
          {t("wordLists.notice")}
        </p>
        <Button type="button" onClick={() => open()}>
          {t("wordLists.new")}
        </Button>
        {loading && <p role="status">{t("wordLists.loading")}</p>}
        {loadFailed && <p role="alert">{t("wordLists.loadFailed")}</p>}
        {!loading && owned.length === 0 && <p className="setting-note">{t("wordLists.empty")}</p>}
        <ul className="layout-list" aria-label={t("wordLists.list")}>
          {owned.map((row) => (
            <li key={row.saved.wordListId}>
              <span className="layout-list-name">
                {row.saved.name}{" "}
                <small className="setting-note">{t(wordCount(row.saved.wordCount))}</small>
              </span>{" "}
              <span className="layout-list-actions">
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("wordLists.edit", { name: row.saved.name })}
                  onClick={() => open(row.saved.wordListId)}
                >
                  {t("wordLists.editShort")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("wordLists.share", { name: row.saved.name })}
                  onClick={() => void share(row)}
                >
                  {t("wordLists.shareShort")}
                </Button>
                {row.saved.shareId !== undefined && (
                  <Button
                    type="button"
                    variant="outline"
                    aria-label={t("wordLists.unshare", { name: row.saved.name })}
                    onClick={() => void unshare(row)}
                  >
                    {t("wordLists.unshareShort")}
                  </Button>
                )}
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("wordLists.delete", { name: row.saved.name })}
                  onClick={() => void remove(row)}
                >
                  {t("wordLists.deleteShort")}
                </Button>
              </span>
            </li>
          ))}
        </ul>
        {message && <p role={message.alert ? "alert" : "status"}>{t(message.text)}</p>}
        {shared && (
          <div className="layout-share">
            <p>
              <strong>{sharedName}</strong>: <a href={shared.url}>{t("wordLists.shareLink")}</a>
            </p>
            <p className="setting-note">{t("wordLists.shareNote")}</p>
            <CopyLinkButton url={shared.url} />
          </div>
        )}
      </section>
      {editing && (editing.wordListId === undefined || editingRow) && (
        <WordListEditor
          key={editing.key}
          {...(editingRow ? { initial: editingRow.saved } : {})}
          {...(limits ? { limits } : {})}
          onSave={(name, words) => save(editing.wordListId, name, words)}
          onClose={() => setEditing(undefined)}
        />
      )}
    </>
  );
}

type Feedback = { message: Copy; blocking: boolean };
export function WordListEditor({
  initial,
  limits,
  onSave,
  onClose,
}: {
  initial?: OwnedWordList;
  limits?: WordListLimits;
  onSave: (name: string, words: string) => Promise<void>;
  onClose: () => void;
}) {
  const { t } = useCopy();
  const ids = useId();
  const [name, setName] = useState(initial?.name ?? "");
  const [words, setWords] = useState(initial?.words ?? "");
  const [feedback, setFeedback] = useState<Feedback>();
  const [state, setState] = useState<"idle" | "importing" | "saving" | "saved">("idle");
  const busy = state === "importing" || state === "saving";

  function edited() {
    setFeedback(undefined);
    if (!busy) {
      setState("idle");
    }
  }
  async function choose(event: ChangeEvent<HTMLInputElement>) {
    const input = event.currentTarget;
    const file = input.files?.[0];
    if (!file) {
      return;
    }
    setFeedback(undefined);
    setState("importing");
    const result = await importWordListFile(file, limits);
    input.value = "";
    setState("idle");
    if (result.kind === "text") {
      setWords(result.text);
      return;
    }
    // A rejected file never leaves partial words behind to save by accident.
    setWords("");
    const message: Copy =
      result.kind === "tooLarge"
        ? { key: "wordEditor.tooLarge", values: sizeValues(limits) }
        : { key: result.kind === "notText" ? "wordEditor.notText" : "wordEditor.fileEmpty" };
    setFeedback({ message, blocking: true });
  }
  async function save() {
    if (busy || feedback?.blocking) {
      return;
    }
    if (
      name.length === 0 ||
      name.trim() !== name ||
      Array.from(name).length > 64 ||
      /\p{Cc}/u.test(name)
    ) {
      setFeedback({ message: { key: "wordEditor.nameInvalid" }, blocking: false });
      return;
    }
    if (words.trim() === "") {
      setFeedback({ message: { key: "wordEditor.empty" }, blocking: false });
      return;
    }
    if (limits && new TextEncoder().encode(words).length > limits.maxBytes) {
      setFeedback({
        message: { key: "wordEditor.tooLarge", values: sizeValues(limits) },
        blocking: false,
      });
      return;
    }
    setFeedback(undefined);
    setState("saving");
    try {
      await onSave(name, words);
      setState("saved");
    } catch (error) {
      setState("idle");
      setFeedback({ message: wordListFailure(error, limits), blocking: false });
    }
  }

  return (
    <section className="layout-editor" aria-label={t("wordEditor.region")}>
      <h2>
        {initial ? t("wordEditor.editTitle", { name: initial.name }) : t("wordEditor.newTitle")}
      </h2>
      <label className="layout-editor-field">
        {t("wordEditor.name")}
        <input
          type="text"
          value={name}
          maxLength={64}
          onChange={(event) => {
            edited();
            setName(event.currentTarget.value);
          }}
        />
      </label>
      <label className="layout-editor-field">
        {t("wordEditor.file")}
        <input
          type="file"
          accept=".txt,text/plain"
          aria-describedby={`${ids}-file-help`}
          disabled={state === "saving"}
          onChange={(event) => void choose(event)}
        />
      </label>
      <small id={`${ids}-file-help`} className="setting-note">
        {t("wordEditor.fileHelp", sizeValues(limits))}
      </small>
      <label className="layout-editor-field word-list-words">
        {t("wordEditor.words")}
        <textarea
          value={words}
          rows={10}
          spellCheck={false}
          autoCapitalize="characters"
          aria-describedby={`${ids}-words-help`}
          onChange={(event) => {
            edited();
            setWords(event.currentTarget.value);
          }}
        />
      </label>
      <small id={`${ids}-words-help`} className="setting-note">
        {t("wordEditor.wordsHelp")}
      </small>
      {feedback && <p role="alert">{t(feedback.message)}</p>}
      <p role="status">{state === "saved" ? t("wordEditor.saved") : ""}</p>
      <div className="layout-editor-actions">
        <Button
          type="button"
          disabled={busy || feedback?.blocking === true}
          onClick={() => void save()}
        >
          {t("wordEditor.save")}
        </Button>
        <Button type="button" variant="outline" onClick={onClose}>
          {t("editor.close")}
        </Button>
      </div>
    </section>
  );
}
