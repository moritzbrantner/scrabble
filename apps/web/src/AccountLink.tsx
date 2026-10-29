// Sign-in, link and reconnect for guest-owned libraries (#132; Decision 1.46). Sign-in
// itself happens on the shared Moenarch account page; Scrabble only asks its own API
// who the browser's account session belongs to.
import { Button } from "@moritzbrantner/ui/client";
import { useCallback, useEffect, useState } from "react";

import {
  AccountError,
  accountSession,
  linkLocalLibraries,
  localOwners,
  reconnectLinkedLibraries,
  type AccountSession,
} from "./account-link";
import { type CopyKey, type Values } from "./copy";
import { useCopy } from "./preferences";

type State =
  | { kind: "checking" }
  | { kind: "off" }
  | { kind: "signedOut"; signIn?: string }
  | { kind: "signedIn"; session: AccountSession }
  | { kind: "unavailable" };

/** `onReconnected` runs after device capabilities were added, so libraries reload. */
export function AccountLink({ api, onReconnected }: { api: string; onReconnected: () => void }) {
  const { t } = useCopy();
  const [state, setState] = useState<State>({ kind: "checking" });
  const [busy, setBusy] = useState(false);
  const [messages, setMessages] = useState<{ key: CopyKey; values?: Values; alert?: boolean }[]>(
    [],
  );

  const check = useCallback(async () => {
    setState({ kind: "checking" });
    try {
      setState({ kind: "signedIn", session: await accountSession(api) });
    } catch (error) {
      if (error instanceof AccountError && error.code === "signedOut") {
        setState(
          error.signIn ? { kind: "signedOut", signIn: error.signIn } : { kind: "signedOut" },
        );
      } else if (error instanceof AccountError && error.code === "unavailable") {
        // Unconfigured servers answer the same way; offer nothing beyond guest use.
        setState({ kind: "off" });
      } else {
        setState({ kind: "unavailable" });
      }
    }
  }, [api]);
  useEffect(() => {
    void check();
  }, [check]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setMessages([]);
    try {
      await action();
    } catch (error) {
      if (error instanceof AccountError && error.code === "signedOut") {
        setState(
          error.signIn ? { kind: "signedOut", signIn: error.signIn } : { kind: "signedOut" },
        );
      } else {
        setMessages([
          {
            key:
              error instanceof AccountError && error.code === "unavailable"
                ? "account.unavailable"
                : "account.failed",
            alert: true,
          },
        ]);
      }
    } finally {
      setBusy(false);
    }
  }
  const link = () =>
    run(async () => {
      if (localOwners(api).length === 0) {
        setMessages([{ key: "account.nothingToLink" }]);
        return;
      }
      const summary = await linkLocalLibraries(api);
      setMessages([
        { key: "account.linked", values: { linked: summary.linked, existing: summary.existing } },
        ...(summary.conflicts > 0
          ? [
              {
                key: "account.conflict" as const,
                values: { count: summary.conflicts },
                alert: true,
              },
            ]
          : []),
      ]);
    });
  const reconnect = () =>
    run(async () => {
      const count = await reconnectLinkedLibraries(api);
      setMessages([
        count > 0 ? { key: "account.reconnected", values: { count } } : { key: "account.upToDate" },
      ]);
      if (count > 0) {
        onReconnected();
      }
    });

  if (state.kind === "off") {
    return null;
  }
  return (
    <section className="layout-library" aria-label={t("account.region")}>
      <h2>{t("account.region")}</h2>
      {state.kind === "checking" && <p role="status">{t("account.checking")}</p>}
      {state.kind === "unavailable" && <p role="alert">{t("account.unavailable")}</p>}
      {state.kind === "signedOut" && (
        <>
          <p className="setting-note">{t("account.signedOut")}</p>
          {state.signIn && (
            <p>
              <a href={state.signIn} target="_blank" rel="noopener">
                {t("account.signIn")}
              </a>
            </p>
          )}
          <Button type="button" variant="outline" onClick={() => void check()}>
            {t("account.recheck")}
          </Button>
        </>
      )}
      {state.kind === "signedIn" && (
        <>
          <p>{t("account.signedIn", { name: state.session.displayName })}</p>
          <p className="setting-note">{t("account.linkNote")}</p>
          <Button type="button" disabled={busy} onClick={() => void link()}>
            {t("account.link")}
          </Button>
          <Button type="button" variant="outline" disabled={busy} onClick={() => void reconnect()}>
            {t("account.reconnect")}
          </Button>
        </>
      )}
      {messages.map((message) => (
        <p key={message.key} role={message.alert ? "alert" : "status"}>
          {t(message.key, message.values)}
        </p>
      ))}
    </section>
  );
}
