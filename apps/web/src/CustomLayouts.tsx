// Guest-owned custom board layout library and editor (#101).
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

import { hexBoardStyle, hexCellStyle } from "./board-geometry";
import { type Copy, type CopyKey } from "./copy";
import {
  boardRow,
  cells,
  contains,
  createLayout,
  deleteLayout,
  forget,
  LayoutError,
  ownedEntries,
  readLayout,
  reshape,
  shareLayout,
  shareUrl,
  updateLayout,
  validSize,
  type BoardRow,
  type LayoutDocument,
  type OwnerEntry,
  type Premium,
  type SavedLayout,
} from "./layout-library";
import { useCopy } from "./preferences";

export type BoardChoice = { value: string; row: BoardRow };
type Owned = { entry: OwnerEntry; saved: SavedLayout };
const premiumKinds = [
  "normal",
  "double_letter",
  "triple_letter",
  "double_word",
  "triple_word",
] as const satisfies readonly Premium[];

export function layoutFailure(error: unknown): Copy {
  if (error instanceof LayoutError) {
    if (error.code === "invalid") {
      return { key: "editor.rejected", values: { reason: error.reason ?? "" } };
    }
    if (error.code === "full") {
      return { key: "layouts.full" };
    }
    if (error.code === "storage") {
      return { key: "layouts.storage" };
    }
  }
  return { key: "layouts.failed" };
}

/** The create page's library region plus the editor; reports saved rows for match setup. */
export function CustomLayouts({
  api,
  onChoices,
}: {
  api: string;
  onChoices: (choices: BoardChoice[]) => void;
}) {
  const { t } = useCopy();
  const [owned, setOwned] = useState<Owned[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [editing, setEditing] = useState<{ key: number; layoutId?: string }>();
  const [shared, setShared] = useState<{ layoutId: string; url: string }>();
  const [message, setMessage] = useState<{ text: CopyKey; alert: boolean }>();
  const editorKey = useRef(0);

  useEffect(() => {
    let active = true;
    const entries = ownedEntries(api);
    void Promise.all(
      entries.map(async (entry) => {
        try {
          return { entry, saved: await readLayout(api, entry) };
        } catch (error) {
          if (error instanceof LayoutError && error.code === "unknown") {
            // Only the issuing API is ever asked, so its answer is authoritative.
            forget(entry);
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
      owned.map(({ saved }) => ({
        value: `layout:${saved.layoutId}`,
        row: boardRow(saved.name, saved.board, saved.layout),
      })),
    );
  }, [owned, onChoices]);

  function open(layoutId?: string) {
    editorKey.current += 1;
    setMessage(undefined);
    setEditing({ key: editorKey.current, ...(layoutId === undefined ? {} : { layoutId }) });
  }
  async function save(layoutId: string | undefined, name: string, layout: LayoutDocument) {
    const current = owned.find((row) => row.saved.layoutId === layoutId);
    const saved =
      current === undefined
        ? await createLayout(api, name, layout)
        : await updateLayout(api, current.entry, name, layout);
    const entry = ownedEntries(api).find((row) => row.layoutId === saved.layoutId);
    if (entry) {
      setOwned((rows) =>
        current === undefined
          ? [...rows, { entry, saved }]
          : rows.map((row) => (row.saved.layoutId === saved.layoutId ? { entry, saved } : row)),
      );
    }
    setEditing((value) => (value ? { ...value, layoutId: saved.layoutId } : value));
    return saved;
  }
  async function remove(row: Owned) {
    setMessage(undefined);
    try {
      await deleteLayout(api, row.entry);
    } catch (error) {
      if (!(error instanceof LayoutError && error.code === "unknown")) {
        setMessage({ text: "layouts.failed", alert: true });
        return;
      }
      forget(row.entry);
    }
    setOwned((rows) => rows.filter((item) => item.saved.layoutId !== row.saved.layoutId));
    setEditing((value) => (value?.layoutId === row.saved.layoutId ? undefined : value));
    setShared((value) => (value?.layoutId === row.saved.layoutId ? undefined : value));
    setMessage({ text: "layouts.deleted", alert: false });
  }
  async function share(row: Owned) {
    setMessage(undefined);
    try {
      const id = row.saved.shareId ?? (await shareLayout(api, row.entry));
      setShared({ layoutId: row.saved.layoutId, url: shareUrl(id) });
    } catch {
      setMessage({ text: "layouts.failed", alert: true });
    }
  }
  const editingRow = owned.find((row) => row.saved.layoutId === editing?.layoutId);
  const sharedName = owned.find((row) => row.saved.layoutId === shared?.layoutId)?.saved.name;

  return (
    <>
      <section className="layout-library" aria-label={t("layouts.region")}>
        <h2>{t("layouts.region")}</h2>
        <p role="note" className="setting-note">
          {t("layouts.notice")}
        </p>
        <Button type="button" onClick={() => open()}>
          {t("layouts.new")}
        </Button>
        {loading && <p role="status">{t("layouts.loading")}</p>}
        {loadFailed && <p role="alert">{t("layouts.loadFailed")}</p>}
        {!loading && owned.length === 0 && <p className="setting-note">{t("layouts.empty")}</p>}
        <ul className="layout-list" aria-label={t("layouts.list")}>
          {owned.map((row) => (
            <li key={row.saved.layoutId}>
              <span className="layout-list-name">{row.saved.name}</span>
              <span className="layout-list-actions">
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("layouts.edit", { name: row.saved.name })}
                  onClick={() => open(row.saved.layoutId)}
                >
                  {t("layouts.editShort")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("layouts.share", { name: row.saved.name })}
                  onClick={() => void share(row)}
                >
                  {t("layouts.shareShort")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("layouts.delete", { name: row.saved.name })}
                  onClick={() => void remove(row)}
                >
                  {t("layouts.deleteShort")}
                </Button>
              </span>
            </li>
          ))}
        </ul>
        {message && <p role={message.alert ? "alert" : "status"}>{t(message.text)}</p>}
        {shared && (
          <div className="layout-share">
            <p>
              <strong>{sharedName}</strong>: <a href={shared.url}>{t("layouts.shareLink")}</a>
            </p>
            <p className="setting-note">{t("layouts.shareNote")}</p>
            <CopyLinkButton url={shared.url} />
          </div>
        )}
      </section>
      {editing && (editing.layoutId === undefined || editingRow) && (
        <LayoutEditor
          key={editing.key}
          {...(editingRow ? { initial: editingRow.saved } : {})}
          onSave={(name, layout) => save(editing.layoutId, name, layout)}
          onClose={() => setEditing(undefined)}
        />
      )}
    </>
  );
}

function CopyLinkButton({ url }: { url: string }) {
  const { t } = useCopy();
  const [copied, setCopied] = useState(false);
  return (
    <>
      <Button
        type="button"
        variant="outline"
        onClick={() => {
          try {
            void navigator.clipboard
              .writeText(url)
              .then(() => setCopied(true))
              .catch(() => setCopied(false));
          } catch {
            setCopied(false);
          }
        }}
      >
        {t("layouts.copyLink")}
      </Button>
      {copied && <span role="status">{t("layouts.linkCopied")}</span>}
    </>
  );
}

type Shape = "square" | "hexagon";
type Draft = {
  name: string;
  shape: Shape;
  sizeText: string;
  size: number;
  premiums: Premium[];
  policy: "all_word" | "main_word_only";
  metric: "tiles" | "letters";
};
function initialDraft(defaultName: string, initial?: SavedLayout): Draft {
  if (initial) {
    return {
      name: initial.name,
      shape: initial.layout.boardShape,
      sizeText: String(initial.layout.boardSize),
      size: initial.layout.boardSize,
      premiums: [...initial.layout.premiums],
      policy: initial.layout.premiumPolicy ?? "all_word",
      metric: initial.layout.mainWordLength ?? "tiles",
    };
  }
  const size = 15;
  const premiums = cells("square", size).map(() => "normal" as Premium);
  premiums[Math.floor((size * size) / 2)] = "double_word";
  return {
    name: defaultName,
    shape: "square",
    sizeText: String(size),
    size,
    premiums,
    policy: "all_word",
    metric: "tiles",
  };
}

export function LayoutEditor({
  initial,
  onSave,
  onClose,
}: {
  initial?: SavedLayout;
  onSave: (name: string, layout: LayoutDocument) => Promise<SavedLayout>;
  onClose: () => void;
}) {
  const { t } = useCopy();
  const ids = useId();
  const [draft, setDraft] = useState(() => initialDraft(t("editor.newTitle"), initial));
  const [brush, setBrush] = useState<Premium>("double_letter");
  const center = Math.floor(draft.size / 2);
  const [focus, setFocus] = useState({ row: center, column: center });
  const [state, setState] = useState<
    { kind: "idle" | "saving" | "saved" } | { kind: "failed"; message: Copy }
  >({
    kind: "idle",
  });
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const sizeNumber = Number(draft.sizeText);
  const sizeValid = draft.sizeText.trim() !== "" && validSize(sizeNumber);
  const nameValid =
    draft.name.length > 0 &&
    draft.name.trim() === draft.name &&
    Array.from(draft.name).length <= 64 &&
    !/\p{Cc}/u.test(draft.name);
  let problem: CopyKey | undefined;
  if (!sizeValid) {
    problem = "editor.sizeInvalid";
  } else if (!nameValid) {
    problem = "editor.nameInvalid";
  }
  const layoutCells = cells(draft.shape, draft.size);
  const rows = Array.from({ length: draft.size }, (_, row) =>
    layoutCells.filter((cell) => cell.row === row),
  ).filter((row) => row.length > 0);

  function update(next: Draft) {
    setDraft(next);
    if (state.kind !== "saving") {
      setState({ kind: "idle" });
    }
  }
  function regrid(shape: Shape, sizeText: string) {
    const size = Number(sizeText);
    if (sizeText.trim() !== "" && validSize(size)) {
      const premiums = reshape(
        { shape: draft.shape, size: draft.size, premiums: draft.premiums },
        shape,
        size,
      );
      const middle = Math.floor(size / 2);
      if (size !== draft.size || shape !== draft.shape) {
        setFocus({ row: middle, column: middle });
      }
      update({ ...draft, shape, sizeText, size, premiums });
    } else {
      update({ ...draft, shape, sizeText });
    }
  }
  function paint(index: number, row: number, column: number) {
    setFocus({ row, column });
    const premiums = [...draft.premiums];
    premiums[index] = brush;
    update({ ...draft, premiums });
  }
  function move(event: KeyboardEvent<HTMLButtonElement>, row: number, column: number) {
    const delta = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
    }[event.key];
    if (!delta) {
      return;
    }
    event.preventDefault();
    const next = { row: row + delta[0]!, column: column + delta[1]! };
    if (contains(draft.shape, draft.size, next.row, next.column)) {
      setFocus(next);
      buttons.current.get(`${next.row},${next.column}`)?.focus();
    }
  }
  async function save() {
    if (problem || state.kind === "saving") {
      return;
    }
    setState({ kind: "saving" });
    const layout: LayoutDocument = {
      boardShape: draft.shape,
      boardSize: draft.size,
      premiums: draft.premiums,
      ...(draft.shape === "hexagon"
        ? { premiumPolicy: draft.policy, mainWordLength: draft.metric }
        : {}),
    };
    try {
      await onSave(draft.name, layout);
      setState({ kind: "saved" });
    } catch (error) {
      setState({ kind: "failed", message: layoutFailure(error) });
    }
  }
  const hex = draft.shape === "hexagon";
  const focusOnBoard = contains(draft.shape, draft.size, focus.row, focus.column)
    ? focus
    : { row: center, column: center };

  return (
    <section className="layout-editor" aria-label={t("editor.region")}>
      <h2>{initial ? t("editor.editTitle", { name: initial.name }) : t("editor.newTitle")}</h2>
      <label className="layout-editor-field">
        {t("editor.name")}
        <input
          type="text"
          value={draft.name}
          maxLength={64}
          onChange={(event) => update({ ...draft, name: event.currentTarget.value })}
        />
      </label>
      <div className="layout-editor-choice">
        <span id={`${ids}-shape`}>{t("editor.shape")}</span>
        <div role="radiogroup" aria-labelledby={`${ids}-shape`}>
          {(["square", "hexagon"] as const).map((shape) => (
            <label key={shape}>
              <input
                type="radio"
                name={`${ids}-shape`}
                checked={draft.shape === shape}
                onChange={() => regrid(shape, draft.sizeText)}
              />
              {t(shape === "square" ? "editor.square" : "editor.hexagon")}
            </label>
          ))}
        </div>
      </div>
      <label className="layout-editor-field">
        {t("editor.size")}
        <input
          type="number"
          min={3}
          max={15}
          step={2}
          value={draft.sizeText}
          aria-describedby={`${ids}-size-help`}
          onChange={(event) => regrid(draft.shape, event.currentTarget.value)}
        />
      </label>
      <small id={`${ids}-size-help`} className="setting-note">
        {t("editor.sizeHelp")}
      </small>
      {hex && (
        <div className="layout-editor-choice">
          <span id={`${ids}-policy`}>{t("editor.application")}</span>
          <div role="radiogroup" aria-labelledby={`${ids}-policy`}>
            {(["all_word", "main_word_only"] as const).map((policy) => (
              <label key={policy}>
                <input
                  type="radio"
                  name={`${ids}-policy`}
                  checked={draft.policy === policy}
                  onChange={() => update({ ...draft, policy })}
                />
                {t(policy === "all_word" ? "editor.allWords" : "editor.mainWord")}
              </label>
            ))}
          </div>
        </div>
      )}
      <div className="layout-editor-choice">
        <span id={`${ids}-brush`}>{t("editor.premiumType")}</span>
        <div role="radiogroup" aria-labelledby={`${ids}-brush`} className="layout-brushes">
          {premiumKinds.map((kind) => (
            <label key={kind}>
              <input
                type="radio"
                name={`${ids}-brush`}
                checked={brush === kind}
                onChange={() => setBrush(kind)}
              />
              <span
                className={`layout-swatch premium-${kind.replaceAll("_", "-")}`}
                aria-hidden="true"
              >
                {t(`premiumShort.${kind}`)}
              </span>
              {t(`premium.${kind}`)}
            </label>
          ))}
        </div>
      </div>
      <p id={`${ids}-cells-help`} className="setting-note">
        {t("editor.cellsHelp")}
      </p>
      <div
        role="grid"
        aria-label={t("editor.cells")}
        aria-describedby={`${ids}-cells-help`}
        className={`layout-editor-grid ${hex ? "hex" : "square"}`}
        style={hex ? hexBoardStyle(draft.size) : { maxWidth: `${draft.size * 2.6}rem` }}
      >
        {rows.map((row) => (
          <div role="row" key={row[0]!.row} className="layout-editor-row">
            {row.map((cell) => {
              const kind = draft.premiums[cell.index] ?? "normal";
              const isCenter = cell.row === center && cell.column === center;
              const roving = cell.row === focusOnBoard.row && cell.column === focusOnBoard.column;
              return (
                <div
                  role="gridcell"
                  key={cell.column}
                  className="layout-editor-cell"
                  style={hex ? hexCellStyle(draft.size, cell.row, cell.column) : undefined}
                >
                  <button
                    type="button"
                    ref={(element) => {
                      const key = `${cell.row},${cell.column}`;
                      if (element) {
                        buttons.current.set(key, element);
                      } else {
                        buttons.current.delete(key);
                      }
                    }}
                    tabIndex={roving ? 0 : -1}
                    data-row={cell.row}
                    data-column={cell.column}
                    aria-label={t("board.square", {
                      row: cell.row + 1,
                      column: cell.column + 1,
                      description: isCenter
                        ? `${t("profiles.center")} · ${t(`premium.${kind}`)}`
                        : t(`premium.${kind}`),
                    })}
                    className={`board-preview-cell premium-${kind.replaceAll("_", "-")}`}
                    onClick={() => paint(cell.index, cell.row, cell.column)}
                    onKeyDown={(event) => move(event, cell.row, cell.column)}
                    onFocus={() => setFocus({ row: cell.row, column: cell.column })}
                  >
                    <span aria-hidden="true">{isCenter ? "★" : t(`premiumShort.${kind}`)}</span>
                  </button>
                </div>
              );
            })}
          </div>
        ))}
      </div>
      {problem && <p role="alert">{t(problem)}</p>}
      {state.kind === "failed" && <p role="alert">{t(state.message)}</p>}
      <p role="status">{state.kind === "saved" ? t("editor.saved") : ""}</p>
      <div className="layout-editor-actions">
        <Button
          type="button"
          disabled={problem !== undefined || state.kind === "saving"}
          onClick={() => void save()}
        >
          {t("editor.save")}
        </Button>
        <Button type="button" variant="outline" onClick={onClose}>
          {t("editor.close")}
        </Button>
      </div>
    </section>
  );
}
