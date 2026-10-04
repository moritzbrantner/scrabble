import { type Premium } from "./public-state";

export const premiums: Record<Premium, { label: string; short: string }> = {
  normal: { label: "Normal square", short: "" },
  double_letter: { label: "Double letter", short: "DL" },
  triple_letter: { label: "Triple letter", short: "TL" },
  double_word: { label: "Double word", short: "DW" },
  triple_word: { label: "Triple word", short: "TW" },
};
