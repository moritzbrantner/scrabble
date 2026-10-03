import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  root: "apps/web",
  base: "/scrabble/",
  plugins: [tailwindcss()],
  build: {
    outDir: "../../dist",
    emptyOutDir: true,
    rolldownOptions: {
      onwarn(warning, defaultHandler) {
        // This is a client-only application, so React server directives have no role.
        if (warning.code === "MODULE_LEVEL_DIRECTIVE" && warning.message.includes("use client")) {
          return;
        }
        defaultHandler(warning);
      },
    },
  },
});
