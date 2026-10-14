import { lazy, Suspense, type ComponentProps } from "react";
import { useCopy } from "./preferences";
import { type LiveBoard } from "./LiveBoard";

// Invitation/QR and live transport presentation are needed only after opening a match.
const Board = lazy(() => import("./LiveBoard").then((module) => ({ default: module.LiveBoard })));
export function ConnectedBoard(props: ComponentProps<typeof LiveBoard>) {
  const { t } = useCopy();
  return (
    <Suspense fallback={<p role="status">{t("connection.connecting")}</p>}>
      <Board {...props} />
    </Suspense>
  );
}
