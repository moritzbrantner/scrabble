import { z } from "zod";

const duration = z.int().min(1).max(86_400_000);
export const timeControl = z
  .strictObject({
    clock: z.discriminatedUnion("kind", [
      z.strictObject({
        kind: z.literal("cumulative"),
        initial_ms: duration,
        increment_ms: duration.optional(),
      }),
      z.strictObject({ kind: z.literal("per_turn"), allowance_ms: duration }),
    ]),
    on_expiry: z.enum(["pass", "forfeit"]),
  })
  .refine(
    (control) =>
      control.clock.kind !== "cumulative" ||
      control.on_expiry !== "pass" ||
      control.clock.increment_ms !== undefined,
  );
export type TimeControl = z.infer<typeof timeControl>;
export type ClockDraft = {
  enabled: boolean;
  model: "" | "cumulative" | "per_turn";
  consequence: "" | "pass" | "forfeit";
  initial: string;
  increment: string;
  perTurn: string;
};
export const initialClockDraft: ClockDraft = {
  enabled: false,
  model: "",
  consequence: "",
  initial: "",
  increment: "",
  perTurn: "",
};
function milliseconds(text: string) {
  if (text.trim() === "") {
    return undefined;
  }
  const value = Number(text) * 1000;
  // Allow precision to one millisecond without floating-point multiplication noise.
  const rounded = Math.round(value);
  return Number.isFinite(value) && Math.abs(value - rounded) < 0.000001 ? rounded : NaN;
}
export function readTimeControl(draft: ClockDraft): {
  control?: TimeControl;
  error?: "clock.invalid" | "clock.incrementRequired";
} {
  if (!draft.enabled) {
    return {};
  }
  if (
    draft.model === "cumulative" &&
    draft.consequence === "pass" &&
    !(Number(draft.increment) > 0)
  ) {
    return { error: "clock.incrementRequired" };
  }
  const increment = milliseconds(draft.increment);
  const clock =
    draft.model === "cumulative"
      ? {
          kind: "cumulative",
          initial_ms: milliseconds(draft.initial),
          ...(increment === undefined ? {} : { increment_ms: increment }),
        }
      : { kind: draft.model, allowance_ms: milliseconds(draft.perTurn) };
  const parsed = timeControl.safeParse({ clock, on_expiry: draft.consequence });
  return parsed.success ? { control: parsed.data } : { error: "clock.invalid" };
}
