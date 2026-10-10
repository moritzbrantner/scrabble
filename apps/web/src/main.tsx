import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { PreferencesProvider, initialPreferences } from "./preferences";

import "./styles.css";

const root = document.getElementById("root");
if (root === null) {
  throw new Error("Missing application root");
}
createRoot(root).render(
  <StrictMode>
    <PreferencesProvider initial={initialPreferences()}>
      <App />
    </PreferencesProvider>
  </StrictMode>,
);
