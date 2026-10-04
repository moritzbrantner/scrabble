import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

for (const { directory, message } of [
  {
    directory: "pages-unconfigured",
    message: "The game service is unavailable. Please try again later.",
  },
  { directory: "pages-invalid", message: "Invalid game connection settings." },
]) {
  test(`production ${directory} fails visibly without a fixture game or certificate pin`, async ({
    page,
  }) => {
    const build = fileURLToPath(new URL(`../../../test-results/${directory}/`, import.meta.url));
    for (const file of readdirSync(join(build, "assets"))) {
      if (file.endsWith(".js")) {
        expect(readFileSync(join(build, "assets", file), "utf8")).not.toContain(
          "private-certificate-canary",
        );
      }
    }
    // Serve the actual, unchanged production artifact through the owned preview origin.
    await page.route("**/scrabble/**", async (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path === "/scrabble/") {
        await route.fulfill({ path: join(build, "index.html") });
      } else if (/^\/scrabble\/assets\/[A-Za-z0-9_.-]+\.(js|css)$/.test(path)) {
        await route.fulfill({ path: join(build, "assets", path.slice(path.lastIndexOf("/") + 1)) });
      } else {
        await route.continue();
      }
    });
    await page.goto(
      "./?fixture=playing&server=https://override.example/game&api=https://override.example/api",
    );
    await expect(page.getByRole("alert")).toHaveText(message);
    await expect(page.getByRole("table", { name: "Scrabble board" })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Create game", exact: true })).toHaveCount(0);
    await expect(page.locator(".fixture-note")).toHaveCount(0);
  });
}
