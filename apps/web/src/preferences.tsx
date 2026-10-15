import { NativeSelect, NativeSelectOption } from "@moritzbrantner/ui/client";
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
  type ChangeEvent,
  type KeyboardEvent,
} from "react";
import { z } from "zod";

import { formatNumber, translate, type Copy, type CopyKey, type Values } from "./copy";

const preferences = z.strictObject({
  locale: z.enum(["en", "de", "es"]),
  theme: z.enum(["light", "dark", "system"]),
});
type Preferences = z.infer<typeof preferences>;
const storageKey = "scrabble:preferences:v1";
const fallback: Preferences = { locale: "en", theme: "system" };
const PreferencesContext = createContext(fallback);

export function initialPreferences(): Preferences {
  let saved: Preferences | undefined;
  try {
    const result = preferences.safeParse(JSON.parse(localStorage.getItem(storageKey) ?? "null"));
    if (result.success) {
      saved = result.data;
    }
  } catch {
    // Restricted storage must not prevent playing or selecting preferences for this page.
  }
  const query = new URLSearchParams(window.location.search).getAll("lang");
  const requested =
    query.length === 1 ? query[0] : (saved?.locale ?? navigator.language.split("-")[0]);
  const locale = preferences.shape.locale.safeParse(requested);
  return { locale: locale.success ? locale.data : "en", theme: saved?.theme ?? "system" };
}
export function useCopy() {
  const { locale } = useContext(PreferencesContext);
  return {
    locale,
    t: (message: Copy | CopyKey, values?: Values) => translate(locale, message, values),
    number: (value: number | bigint, signDisplay: "auto" | "exceptZero" = "auto") =>
      formatNumber(locale, value, signDisplay === "exceptZero"),
  };
}
export function PreferencesProvider({
  children,
  initial,
}: {
  children: ReactNode;
  initial: Preferences;
}) {
  const [value, setValue] = useState(initial);
  const [storageFailed, setStorageFailed] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const languageId = useId();
  const themeId = useId();
  const titleId = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const gearRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    document.documentElement.lang = value.locale;
  }, [value.locale]);
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = value.theme === "dark" || (value.theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
      document.documentElement.style.colorScheme = dark ? "dark" : "light";
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [value.theme]);
  function update(next: Preferences) {
    setValue(next);
    try {
      localStorage.setItem(storageKey, JSON.stringify(next));
      setStorageFailed(false);
    } catch {
      setStorageFailed(true);
    }
    if (next.locale !== value.locale) {
      const location = new URL(window.location.href);
      location.searchParams.set("lang", next.locale);
      window.history.replaceState(null, "", location);
    }
  }
  const t = (key: CopyKey) => translate(value.locale, key);
  const storageAlert = storageFailed && <p role="alert">{t("settings.storage")}</p>;
  function openSettings() {
    const dialog = dialogRef.current;
    if (dialog !== null && !dialog.open) {
      dialog.showModal();
      setSettingsOpen(true);
    }
  }
  return (
    <PreferencesContext value={value}>
      <aside className="app-preferences" aria-label={t("settings.title")}>
        <div className="preference-field">
          <label className="visually-hidden" htmlFor={languageId}>
            {t("settings.language")}
          </label>
          <NativeSelect
            id={languageId}
            value={value.locale}
            onChange={(event: ChangeEvent<HTMLSelectElement>) => {
              const parsed = preferences.shape.locale.safeParse(event.target.value);
              if (parsed.success) {
                update({ ...value, locale: parsed.data });
              }
            }}
          >
            {(["en", "de", "es"] as const).map((locale) => (
              <NativeSelectOption key={locale} value={locale}>
                {{ en: "English", de: "Deutsch", es: "Español" }[locale]}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </div>
        <div className="preference-field">
          <label className="visually-hidden" htmlFor={themeId}>
            {t("settings.theme")}
          </label>
          <NativeSelect
            id={themeId}
            value={value.theme}
            onChange={(event: ChangeEvent<HTMLSelectElement>) => {
              const parsed = preferences.shape.theme.safeParse(event.target.value);
              if (parsed.success) {
                update({ ...value, theme: parsed.data });
              }
            }}
          >
            {(["light", "dark", "system"] as const).map((theme) => (
              <NativeSelectOption key={theme} value={theme}>
                {t(`settings.${theme}`)}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </div>
        <button
          ref={gearRef}
          type="button"
          className="settings-gear"
          aria-label={t("settings.title")}
          aria-haspopup="dialog"
          aria-expanded={settingsOpen}
          onClick={openSettings}
        >
          <GearIcon />
        </button>
        {!settingsOpen && storageAlert}
      </aside>
      {children}
      <dialog
        ref={dialogRef}
        className="settings-dialog"
        aria-labelledby={titleId}
        aria-modal="true"
        onClose={() => {
          setSettingsOpen(false);
          gearRef.current?.focus();
        }}
        onKeyDown={(event: KeyboardEvent<HTMLDialogElement>) => {
          if (event.key === "Escape") {
            // Settings own Escape while open; gameplay keyboard handling never sees it.
            event.stopPropagation();
          } else if (event.key === "Tab") {
            trapFocus(event);
          }
        }}
      >
        <h2 id={titleId}>{t("settings.title")}</h2>
        <p>{t("settings.note")}</p>
        {settingsOpen && storageAlert}
        <button type="button" className="settings-close" onClick={() => dialogRef.current?.close()}>
          {t("settings.close")}
        </button>
      </dialog>
    </PreferencesContext>
  );
}

/** Keeps Tab and Shift+Tab cycling inside the open modal. */
function trapFocus(event: KeyboardEvent<HTMLDialogElement>) {
  const focusable = Array.from(
    event.currentTarget.querySelectorAll<HTMLElement>(
      "button:not([disabled]), select:not([disabled]), input:not([disabled]), a[href], [tabindex]:not([tabindex='-1'])",
    ),
  );
  const first = focusable.at(0);
  const last = focusable.at(-1);
  if (first === undefined || last === undefined) {
    event.preventDefault();
    return;
  }
  const active = document.activeElement;
  if (event.shiftKey && (active === first || !event.currentTarget.contains(active))) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && (active === last || !event.currentTarget.contains(active))) {
    event.preventDefault();
    first.focus();
  }
}

function GearIcon() {
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}
