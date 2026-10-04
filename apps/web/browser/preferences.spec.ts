import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("language and explicit themes persist; system theme follows the browser", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("./?fixture=playing&lang=de");
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  await expect(page.getByRole("table", { name: "Scrabble-Spielbrett" })).toBeVisible();
  expect(await page.evaluate(() => localStorage.length)).toBe(0);
  await page.getByText("Einstellungen", { exact: true }).click();
  await page.getByLabel("Darstellung", { exact: true }).selectOption("dark");
  await expect(page.locator("html")).toHaveClass("dark");
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
  await page.getByLabel("Sprache der Oberfläche", { exact: true }).selectOption("es");
  await expect(page.getByRole("table", { name: "Tablero de Scrabble" })).toBeVisible();
  expect(new URL(page.url()).searchParams.get("lang")).toBe("es");
  expect(
    await page.evaluate(() =>
      JSON.parse(localStorage.getItem("scrabble:preferences:v1") ?? "null"),
    ),
  ).toEqual({ locale: "es", theme: "dark" });
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("lang", "es");
  await expect(page.locator("html")).toHaveClass("dark");
  await page.getByText("Ajustes", { exact: true }).click();
  await page.getByLabel("Apariencia", { exact: true }).selectOption("system");
  await expect(page.locator("html")).not.toHaveClass("dark");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(page.locator("html")).toHaveClass("dark");
  await page.getByLabel("Apariencia", { exact: true }).selectOption("light");
  await expect(page.locator("html")).not.toHaveClass("dark");
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test("unsupported language and corrupt preferences fall back to English", async ({ page }) => {
  await page.addInitScript(() =>
    localStorage.setItem(
      "scrabble:preferences:v1",
      JSON.stringify({ locale: "secret", theme: "dark", token: "private" }),
    ),
  );
  await page.goto("./?fixture=lobby&lang=fr");
  await expect(page.locator("html")).toHaveAttribute("lang", "en");
  await expect(page.getByRole("table", { name: "Scrabble board" })).toBeVisible();
});

test("an existing validation error changes language without a reload", async ({ page }) => {
  await page.goto("./?view=player&match=invalid");
  await expect(page.getByRole("alert")).toBeVisible();
  const before = await page.getByRole("alert").textContent();
  await page.getByText("Settings", { exact: true }).click();
  await page.getByLabel("Interface language", { exact: true }).selectOption("de");
  await expect(page.getByRole("alert")).not.toHaveText(before ?? "");
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});

test("restricted preference storage keeps the page usable and explains persistence", async ({
  page,
}) => {
  await page.addInitScript(() => {
    Storage.prototype.setItem = () => {
      throw new Error("Storage disabled");
    };
  });
  await page.goto("./?fixture=playing");
  await page.getByText("Settings", { exact: true }).click();
  await page.getByLabel("Interface language", { exact: true }).selectOption("de");
  await expect(page.getByRole("table", { name: "Scrabble-Spielbrett" })).toBeVisible();
  await expect(page.getByRole("alert")).toBeVisible();
  await page.getByLabel("Darstellung", { exact: true }).selectOption("dark");
  await expect(page.locator("html")).toHaveClass("dark");
  expect((await new AxeBuilder({ page }).analyze()).violations).toEqual([]);
});
