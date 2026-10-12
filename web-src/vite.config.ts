/**
 * @file Vite build and dev-server configuration for the SolidJS SPA. The build
 * output in `dist` is embedded into the gateway binary, so asset names are
 * fixed.
 */

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
  plugins: [
    solid(),
    tailwindcss(),
    {
      name: "document-embedded-ui",
      // Vite minifies renderChunk output, including Rollup's normal banner.
      // Document each emitted module after that step, without changing code.
      generateBundle(_options, bundle) {
        for (const output of Object.values(bundle)) {
          if (output.type === "chunk") {
            output.code = `/** @file Embedded Axinite SolidJS gateway UI; generated from web-src/axinite. */\n${output.code}`;
          }
        }
      },
    },
  ],
  build: {
    outDir: resolve(__dirname, "dist"),
    emptyOutDir: true,
    // Inline the self-hosted fonts as data URIs so they travel inside the
    // embedded stylesheet rather than needing their own gateway routes.
    assetsInlineLimit: (filePath) =>
      filePath.endsWith(".woff2") ? true : undefined,
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
