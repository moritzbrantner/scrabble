import { expect, test } from "bun:test";

import { connectionSettings } from "./connection-settings";

test("development direct boards need only a valid transport endpoint", () => {
  expect(
    connectionSettings("development", "https://127.0.0.1:4433/game", undefined, "board"),
  ).toEqual({
    kind: "board",
    endpoint: "https://127.0.0.1:4433/game",
  });
  for (const endpoint of [
    undefined,
    "http://game.example/game",
    "https://user:secret@game.example/game",
  ]) {
    expect(connectionSettings("development", endpoint, undefined, "board").kind).toBe(
      "unavailable",
    );
  }
  for (const mode of ["production", "test", "development"] as const) {
    expect(connectionSettings(mode, "https://game.example/game", undefined, "game").kind).toBe(
      "unavailable",
    );
  }
  expect(
    connectionSettings("production", "https://game.example/game", undefined, "board").kind,
  ).toBe("unavailable");
});

test("production requires both secure public service addresses and never previews a fixture", () => {
  expect(connectionSettings("production", undefined, undefined)).toEqual({
    kind: "unavailable",
    message: "app.serviceUnavailable",
  });
  expect(connectionSettings("production", "", "").kind).toBe("unavailable");
  expect(connectionSettings("development", undefined, undefined)).toEqual({ kind: "preview" });
  expect(
    connectionSettings("production", "https://game.example/game", "https://game.example/api"),
  ).toEqual({
    kind: "configured",
    endpoint: "https://game.example/game",
    api: "https://game.example/api",
  });
  for (const [endpoint, api] of [
    ["https://game.example/game", undefined],
    [undefined, "https://game.example/api"],
    ["http://game.example/game", "https://game.example/api"],
    ["https://game.example/game", "http://127.0.0.1:8081"],
    ["https://user:secret@game.example/game", "https://game.example/api"],
    ["https://game.example/game", "https://user:secret@game.example/api"],
    ["https://game.example/game?route=other", "https://game.example/api"],
    ["https://game.example/game", "https://game.example/api#other"],
  ]) {
    expect(connectionSettings("production", endpoint, api)).toEqual({
      kind: "unavailable",
      message: "create.invalidConnection",
    });
  }
  expect(
    connectionSettings("test", "https://127.0.0.1:4433/game", "http://127.0.0.1:8081").kind,
  ).toBe("configured");
  expect(
    connectionSettings("test", "https://game.example/game", "http://game.example/api").kind,
  ).toBe("unavailable");
});
