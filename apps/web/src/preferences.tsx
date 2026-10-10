import { NativeSelect, NativeSelectOption } from "@moritzbrantner/ui/client";
import {
  createContext,
  useContext,
  useEffect,
  useId,
  useState,
  type ReactNode,
  type ChangeEvent,
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
  const languageId = useId();
  const themeId = useId();
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
  return (
    <PreferencesContext value={value}>
      {children}
      <aside className="app-preferences" aria-label={t("settings.title")}>
        <details>
          <summary>{t("settings.title")}</summary>
          <div className="preference-fields">
            <label htmlFor={languageId}>{t("settings.language")}</label>
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
            <label htmlFor={themeId}>{t("settings.theme")}</label>
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
            {storageFailed && <p role="alert">{t("settings.storage")}</p>}
          </div>
        </details>
      </aside>
    </PreferencesContext>
  );
}
