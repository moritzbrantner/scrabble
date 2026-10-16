import { NativeSelect, NativeSelectOption } from "@moritzbrantner/ui/client";
import { type ChangeEvent } from "react";

import { useCopy } from "./preferences";
import { type ClockDraft } from "./time-control";

export function ClockSetup({
  draft,
  onChange,
}: {
  draft: ClockDraft;
  onChange: (draft: ClockDraft) => void;
}) {
  const { t } = useCopy();
  const fields =
    draft.model === "cumulative" ? (["initial", "increment"] as const) : (["perTurn"] as const);
  return (
    <fieldset className="clock-setup">
      <legend>{t("clock.setup")}</legend>
      <label className="clock-enable">
        <input
          type="checkbox"
          checked={draft.enabled}
          onChange={(event) => onChange({ ...draft, enabled: event.currentTarget.checked })}
        />
        {t("clock.enable")}
      </label>
      {draft.enabled && (
        <>
          <p className="setting-note">{t("clock.explanation")}</p>
          <div className="clock-fields">
            <label>
              {t("clock.model")}
              <NativeSelect
                value={draft.model}
                onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                  const model = event.currentTarget.value;
                  if (model === "cumulative" || model === "per_turn") {
                    onChange({ ...draft, model });
                  }
                }}
              >
                <NativeSelectOption value="" disabled>
                  {t("clock.choose")}
                </NativeSelectOption>
                <NativeSelectOption value="cumulative">{t("clock.cumulative")}</NativeSelectOption>
                <NativeSelectOption value="per_turn">{t("clock.perTurn")}</NativeSelectOption>
              </NativeSelect>
            </label>
            <label>
              {t("clock.consequence")}
              <NativeSelect
                value={draft.consequence}
                onChange={(event: ChangeEvent<HTMLSelectElement>) => {
                  const consequence = event.currentTarget.value;
                  if (consequence === "pass" || consequence === "forfeit") {
                    onChange({ ...draft, consequence });
                  }
                }}
              >
                <NativeSelectOption value="" disabled>
                  {t("clock.choose")}
                </NativeSelectOption>
                <NativeSelectOption value="pass">{t("clock.pass")}</NativeSelectOption>
                <NativeSelectOption value="forfeit">{t("clock.forfeit")}</NativeSelectOption>
              </NativeSelect>
            </label>
            {draft.model !== "" &&
              fields.map((field) => (
                <label key={field}>
                  {t(`clock.${field}Label`)}
                  <input
                    aria-label={t(`clock.${field}Label`)}
                    type="number"
                    min="0.001"
                    max="86400"
                    step="0.001"
                    value={draft[field]}
                    onChange={(event) => onChange({ ...draft, [field]: event.currentTarget.value })}
                  />
                  <small className="setting-note">{t(`clock.${field}Help`)}</small>
                </label>
              ))}
          </div>
        </>
      )}
    </fieldset>
  );
}
