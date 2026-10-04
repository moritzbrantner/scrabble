import { expect, test } from "bun:test";
import { catalogs, copy, formatNumber, translate, type CopyKey } from "./copy";

const placeholders = (text: string) =>
  [...text.matchAll(/\{([a-zA-Z]+)\}/g)]
    .map((match) => match[1] ?? "")
    .sort((a, b) => a.localeCompare(b));
test("every language supplies the same keys and parameters", () => {
  const keys = Object.keys(catalogs.en);
  for (const catalog of Object.values(catalogs)) {
    expect(Object.keys(catalog).sort((a, b) => a.localeCompare(b))).toEqual(
      [...keys].sort((a, b) => a.localeCompare(b)),
    );
    for (const key of keys) {
      const parsed = key as CopyKey;
      expect(placeholders(catalog[parsed])).toEqual(placeholders(catalogs.en[parsed]));
    }
  }
});
test("message records translate at display time and format values in the selected language", () => {
  const message = copy("phone.waitPlayer", { name: "Ada" });
  expect(translate("en", message)).toBe("Waiting for Ada.");
  expect(translate("de", message)).toBe("Warten auf Ada.");
  expect(translate("es", message)).toBe("Esperando a Ada.");
  expect(translate("de", "score.points", { count: 1234 })).toBe("1.234 Punkte");
  expect(formatNumber("de", 1234n, true)).toBe("+1.234");
  expect(() => translate("en", "phone.waitPlayer")).toThrow("Missing copy parameter");
});
