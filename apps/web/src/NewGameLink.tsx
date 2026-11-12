import { useCopy } from "./preferences";

/** Re-enter App's existing setup flow without carrying the failed match/share/launch. */
export function NewGameLink() {
  const { t } = useCopy();
  const location = new URL(window.location.href);
  for (const key of ["match", "view", "share", "launch", "session", "fixture"]) {
    location.searchParams.delete(key);
  }
  location.hash = "";
  return <a href={location.href}>{t("app.newGame")}</a>;
}
