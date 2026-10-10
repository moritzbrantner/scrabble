import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const certificate = process.env.LOCAL_TLS_CERT;
const privateKey = process.env.LOCAL_TLS_KEY;
if (Boolean(certificate) !== Boolean(privateKey)) {
  throw new Error("Local HTTPS needs both LOCAL_TLS_CERT and LOCAL_TLS_KEY");
}

export default defineConfig(({ mode }) => ({
  root: "apps/web",
  base: "/scrabble/",
  plugins: [tailwindcss()],
  ...(certificate && privateKey
    ? {
        server: {
          https: { cert: readFileSync(certificate), key: readFileSync(privateKey) },
          fs: {
            deny: ["**/.local/**", ".env", ".env.*", "*.{crt,pem}", "**/.git/**"],
          },
          proxy: {
            "/api": {
              target: "http://127.0.0.1:8081",
              rewrite: (path: string) => path.replace(/^\/api(?=\/|$)/, ""),
            },
          },
        },
      }
    : {}),
  build: {
    outDir: "../../dist",
    emptyOutDir: true,
    rolldownOptions: {
      // Browser budget fixtures run against optimized Vite output, never the
      // development server. Only test builds include the extra HTML entry.
      ...(mode === "test"
        ? {
            input: {
              app: fileURLToPath(new URL("./index.html", import.meta.url)),
              performance: fileURLToPath(new URL("./browser/performance.html", import.meta.url)),
            },
          }
        : {}),
      onwarn(warning, defaultHandler) {
        // This is a client-only application, so React server directives have no role.
        if (warning.code === "MODULE_LEVEL_DIRECTIVE" && warning.message.includes("use client")) {
          return;
        }
        defaultHandler(warning);
      },
    },
  },
}));
