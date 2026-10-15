/** Extra classes for a face wider than one letter, so CSS can shrink it to fit its tile. */
export function faceClass(text: string): string | undefined {
  const length = Array.from(text).length;
  if (length >= 3) {
    return "face-multi face-long";
  }
  return length === 2 ? "face-multi" : undefined;
}
