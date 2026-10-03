import { test, expect } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { App } from "./App";

test("entry point clearly identifies the application and current availability", () => {
  const html = renderToStaticMarkup(<App />);
  expect(html).toContain("Scrabble</h1>");
  expect(html).toContain("not available yet");
});
