// Guest-owned private tile-profile library and editor (#100; Decisions 1.27, 1.29, 1.45).
// The server is the authority on every tile rule; this view edits a table of faces,
// counts and points and reports the server's reason precisely.
import { Button } from "@moritzbrantner/ui/client";
import { useEffect, useRef, useState } from "react";

import { type Copy, type CopyKey } from "./copy";
import { useCopy } from "./preferences";
import {
  createTileProfile,
  deleteTileProfile,
  draftFrom,
  forgetTileProfile,
  ownedTileProfiles,
  readTileProfile,
  TileProfileError,
  tilesFrom,
  updateTileProfile,
  type Draft,
  type SavedTileProfile,
  type TileDefinition,
  type TileProfileOwner,
  type TileRow,
} from "./tile-profile-library";

export type TileChoice = { value: string; row: TileRow };
type Owned = { entry: TileProfileOwner; saved: SavedTileProfile };

function failure(error: unknown): Copy {
  if (error instanceof TileProfileError) {
    if (error.code === "invalid") {
      return { key: "tileEditor.rejected", values: { reason: error.reason ?? "" } };
    }
    if (error.code === "full") {
      return { key: "tileProfiles.full" };
    }
    if (error.code === "storage") {
      return { key: "tileProfiles.storage" };
    }
  }
  return { key: "tileProfiles.failed" };
}

/** The create page's private tile-profile region; reports saved rows for match setup.
 * A new profile starts as a copy of `template`, the currently selected tile profile. */
export function CustomTileProfiles({
  api,
  template,
  onChoices,
}: {
  api: string;
  template: TileRow | undefined;
  onChoices: (choices: TileChoice[]) => void;
}) {
  const { t } = useCopy();
  const [owned, setOwned] = useState<Owned[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [editing, setEditing] = useState<{ key: number; profileId?: string; start: Draft }>();
  const [message, setMessage] = useState<{ text: CopyKey; alert: boolean }>();
  const editorKey = useRef(0);

  useEffect(() => {
    let active = true;
    void Promise.all(
      ownedTileProfiles(api).map(async (entry) => {
        try {
          return { entry, saved: await readTileProfile(api, entry) };
        } catch (error) {
          if (error instanceof TileProfileError && error.code === "unknown") {
            forgetTileProfile(entry);
          } else if (active) {
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
        value: `tiles:${saved.profileId}`,
        row: { identity: saved.identity, label: saved.name, tiles: saved.tiles },
      })),
    );
  }, [owned, onChoices]);

  function open(start: Draft, profileId?: string) {
    editorKey.current += 1;
    setMessage(undefined);
    setEditing({
      key: editorKey.current,
      start,
      ...(profileId === undefined ? {} : { profileId }),
    });
  }
  async function save(profileId: string | undefined, name: string, tiles: TileDefinition[]) {
    const current = owned.find((row) => row.saved.profileId === profileId);
    if (current) {
      const saved = await updateTileProfile(api, current.entry, name, tiles);
      setOwned((rows) =>
        rows.map((row) => (row.saved.profileId === saved.profileId ? { ...row, saved } : row)),
      );
      return;
    }
    const created = await createTileProfile(api, name, tiles);
    setOwned((rows) => [...rows, created]);
    setEditing((value) => (value ? { ...value, profileId: created.saved.profileId } : value));
  }
  async function remove(row: Owned) {
    if (!window.confirm(t("tileProfiles.deleteConfirm", { name: row.saved.name }))) {
      return;
    }
    setMessage(undefined);
    try {
      await deleteTileProfile(api, row.entry);
    } catch (error) {
      if (!(error instanceof TileProfileError && error.code === "unknown")) {
        setMessage({ text: "tileProfiles.failed", alert: true });
        return;
      }
      forgetTileProfile(row.entry);
    }
    const id = row.saved.profileId;
    setOwned((rows) => rows.filter((item) => item.saved.profileId !== id));
    setEditing((value) => (value?.profileId === id ? undefined : value));
    setMessage({ text: "tileProfiles.deleted", alert: false });
  }

  return (
    <>
      <section className="layout-library" aria-label={t("tileProfiles.region")}>
        <h2>{t("tileProfiles.region")}</h2>
        <p role="note" className="setting-note">
          {t("tileProfiles.notice")}
        </p>
        <Button
          type="button"
          disabled={template === undefined}
          onClick={() =>
            template &&
            open(draftFrom(t("tileProfiles.copyName", { name: template.label }), template.tiles))
          }
        >
          {t("tileProfiles.new")}
        </Button>
        <p className="setting-note">{t("tileProfiles.newNote")}</p>
        {loading && <p role="status">{t("tileProfiles.loading")}</p>}
        {loadFailed && <p role="alert">{t("tileProfiles.loadFailed")}</p>}
        {!loading && owned.length === 0 && (
          <p className="setting-note">{t("tileProfiles.empty")}</p>
        )}
        <ul className="layout-list" aria-label={t("tileProfiles.list")}>
          {owned.map((row) => (
            <li key={row.saved.profileId}>
              <span className="layout-list-name">{row.saved.name}</span>
              <span className="layout-list-actions">
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("tileProfiles.edit", { name: row.saved.name })}
                  onClick={() =>
                    open(draftFrom(row.saved.name, row.saved.tiles), row.saved.profileId)
                  }
                >
                  {t("tileProfiles.editShort")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  aria-label={t("tileProfiles.delete", { name: row.saved.name })}
                  onClick={() => void remove(row)}
                >
                  {t("tileProfiles.deleteShort")}
                </Button>
              </span>
            </li>
          ))}
        </ul>
        {message && <p role={message.alert ? "alert" : "status"}>{t(message.text)}</p>}
      </section>
      {editing && (
        <TileProfileEditor
          key={editing.key}
          start={editing.start}
          title={
            editing.profileId === undefined
              ? t("tileEditor.newTitle")
              : t("tileEditor.editTitle", { name: editing.start.name })
          }
          onSave={(name, tiles) => save(editing.profileId, name, tiles)}
          onClose={() => setEditing(undefined)}
        />
      )}
    </>
  );
}

function TileProfileEditor({
  start,
  title,
  onSave,
  onClose,
}: {
  start: Draft;
  title: string;
  onSave: (name: string, tiles: TileDefinition[]) => Promise<void>;
  onClose: () => void;
}) {
  const { t, number } = useCopy();
  const [draft, setDraft] = useState(start);
  const [state, setState] = useState<
    { kind: "idle" | "saving" | "saved" } | { kind: "failed"; message: Copy }
  >({ kind: "idle" });
  const nameValid =
    draft.name.length > 0 &&
    draft.name.trim() === draft.name &&
    Array.from(draft.name).length <= 64 &&
    !/\p{Cc}/u.test(draft.name);
  const converted = tilesFrom(draft);
  let problem: Copy | undefined;
  if (!nameValid) {
    problem = { key: "tileEditor.nameInvalid" };
  } else if ("row" in converted) {
    problem = { key: "tileEditor.rowInvalid", values: { row: converted.row } };
  } else if ("blanks" in converted) {
    problem = { key: "tileEditor.blanksInvalid" };
  }
  const total =
    "tiles" in converted ? converted.tiles.reduce((sum, tile) => sum + tile.count, 0) : undefined;

  function update(next: Draft) {
    setDraft(next);
    if (state.kind !== "saving") {
      setState({ kind: "idle" });
    }
  }
  function setRow(index: number, field: "face" | "count" | "value", value: string) {
    update({
      ...draft,
      rows: draft.rows.map((row, at) => (at === index ? { ...row, [field]: value } : row)),
    });
  }
  async function save() {
    if (problem || !("tiles" in converted) || state.kind === "saving") {
      return;
    }
    setState({ kind: "saving" });
    try {
      await onSave(draft.name, converted.tiles);
      setState({ kind: "saved" });
    } catch (error) {
      setState({ kind: "failed", message: failure(error) });
    }
  }

  return (
    <section className="layout-editor" aria-label={t("tileEditor.region")}>
      <h2>{title}</h2>
      <label className="layout-editor-field">
        {t("tileEditor.name")}
        <input
          type="text"
          value={draft.name}
          maxLength={64}
          onChange={(event) => update({ ...draft, name: event.currentTarget.value })}
        />
      </label>
      <p className="setting-note">{t("tileEditor.note")}</p>
      <table className="tile-editor-table">
        <caption>{t("tileEditor.table")}</caption>
        <thead>
          <tr>
            <th scope="col">{t("tileEditor.face")}</th>
            <th scope="col">{t("tileEditor.count")}</th>
            <th scope="col">{t("tileEditor.points")}</th>
          </tr>
        </thead>
        <tbody>
          {draft.rows.map((row, index) => {
            const position = { row: index + 1 };
            return (
              // Rows have no stable identity of their own while being edited.
              // oxlint-disable-next-line react/no-array-index-key
              <tr key={index}>
                <td>
                  <input
                    type="text"
                    aria-label={t("tileEditor.faceRow", position)}
                    value={row.face}
                    maxLength={4}
                    onChange={(event) => setRow(index, "face", event.currentTarget.value)}
                  />
                </td>
                <td>
                  <input
                    type="number"
                    min={0}
                    step={1}
                    aria-label={t("tileEditor.countRow", position)}
                    value={row.count}
                    onChange={(event) => setRow(index, "count", event.currentTarget.value)}
                  />
                </td>
                <td>
                  <input
                    type="number"
                    min={0}
                    step={1}
                    aria-label={t("tileEditor.pointsRow", position)}
                    value={row.value}
                    onChange={(event) => setRow(index, "value", event.currentTarget.value)}
                  />
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <Button
        type="button"
        variant="outline"
        onClick={() =>
          update({ ...draft, rows: [...draft.rows, { face: "", count: "1", value: "1" }] })
        }
      >
        {t("tileEditor.add")}
      </Button>
      <label className="layout-editor-field">
        {t("tileEditor.blanks")}
        <input
          type="number"
          min={0}
          step={1}
          value={draft.blanks}
          onChange={(event) => update({ ...draft, blanks: event.currentTarget.value })}
        />
      </label>
      {total !== undefined && (
        <p className="setting-note">{t("tileEditor.total", { count: number(total) })}</p>
      )}
      {problem && <p role="alert">{t(problem)}</p>}
      {state.kind === "failed" && <p role="alert">{t(state.message)}</p>}
      <p role="status">{state.kind === "saved" ? t("tileEditor.saved") : ""}</p>
      <div className="layout-editor-actions">
        <Button
          type="button"
          disabled={problem !== undefined || state.kind === "saving"}
          onClick={() => void save()}
        >
          {t("tileEditor.save")}
        </Button>
        <Button type="button" variant="outline" onClick={onClose}>
          {t("tileEditor.close")}
        </Button>
      </div>
    </section>
  );
}
