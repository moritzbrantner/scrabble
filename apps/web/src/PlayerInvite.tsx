import { Button } from "@moritzbrantner/ui/client";
import { useMemo, useState } from "react";

import { inviteQr, playerInviteUrl } from "./player-invite";
import { useCopy } from "./preferences";

export function PlayerInvite({ matchId }: { matchId: string }) {
  const { t, locale } = useCopy();
  const [copy, setCopy] = useState<"idle" | "copied" | "failed">("idle");
  const invitation = useMemo(() => {
    try {
      const url = playerInviteUrl(
        new URL(window.location.href),
        matchId,
        Boolean(import.meta.env.DEV),
        import.meta.env.BASE_URL,
      );
      return { url: url.href, qr: inviteQr(url.href) };
    } catch {
      return undefined;
    }
  }, [matchId, locale]);
  if (invitation === undefined) {
    return <p role="alert">{t("invite.failed")}</p>;
  }
  const url = invitation.url;
  async function copyLink() {
    try {
      await navigator.clipboard.writeText(url);
      setCopy("copied");
    } catch {
      setCopy("failed");
    }
  }
  return (
    <section className="player-invite" aria-label={t("invite.title")}>
      <svg
        xmlns="http://www.w3.org/2000/svg"
        role="img"
        aria-label={t("invite.scan")}
        viewBox={`0 0 ${invitation.qr.size} ${invitation.qr.size}`}
        width={invitation.qr.width}
        height={invitation.qr.width}
        shapeRendering="crispEdges"
      >
        <rect width={invitation.qr.size} height={invitation.qr.size} fill="#fff" />
        <path d={invitation.qr.path} fill="#000" />
      </svg>
      <div>
        <a href={invitation.url}>{t("invite.link")}</a>
        <Button onClick={() => void copyLink()}>{t("invite.copy")}</Button>
        {copy === "copied" && <p role="status">{t("invite.copied")}</p>}
        {copy === "failed" && <p role="alert">{t("invite.copyFailed")}</p>}
      </div>
    </section>
  );
}
