import { resolve } from "node:path";

import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

import { DEPLOY_BASE_PATH } from "./axinite/src/lib/base-path";

const projectRoot = resolve(__dirname, "axinite");

// The build output is embedded into the Axinite gateway binary via
// `include_str!`/`include_bytes!`, which needs a fixed file list. Emit
// stable, hash-free artefact names; the gateway serves them with
// `Cache-Control: no-cache`, so content hashing is unnecessary.
export default defineConfig({
  base: DEPLOY_BASE_PATH,
  root: projectRoot,
  publicDir: resolve(projectRoot, "public"),
  plugins: [solid(), tailwindcss()],
  build: {
    outDir: resolve(__dirname, "dist"),
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: "assets/app.js",
        chunkFileNames: "assets/chunk-[name].js",
        assetFileNames: "assets/[name][extname]",
      },
    },
  },
  server: {
    port: 5173,
    host: "0.0.0.0",
  },
  preview: {
    port: 4173,
    host: "0.0.0.0",
  },
  resolve: {
    alias: {
      "@": resolve(__dirname, "axinite/src"),
    },
  },
});
