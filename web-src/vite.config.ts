import { resolve } from "node:path";

import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";
import { VitePWA } from "vite-plugin-pwa";
import solid from "vite-plugin-solid";

import { GITHUB_PAGES_BASE_PATH } from "./axinite/src/lib/base-path";
// @ts-expect-error TypeScript has no ambient declarations for this ESM helper.
import { postbuildRoutes } from "./scripts/postbuild-routes.mjs";

const projectRoot = resolve(__dirname, "axinite");
const deployBasePath = GITHUB_PAGES_BASE_PATH;

export default defineConfig({
  base: deployBasePath,
  root: projectRoot,
  publicDir: resolve(projectRoot, "public"),
  plugins: [
    solid(),
    tailwindcss(),
    {
      name: "postbuild-routes",
      apply: "build",
      async closeBundle() {
        await postbuildRoutes();
      },
    },
    VitePWA({
      registerType: "autoUpdate",
      strategies: "generateSW",
      includeAssets: ["assets/icons/axinite32.ico"],
      workbox: {
        globPatterns: ["**/*.{js,css,html,ico,svg,webmanifest,ftl}"],
        navigateFallback: `${deployBasePath}index.html`,
      },
      manifest: {
        id: deployBasePath,
        name: "Axinite",
        short_name: "Axinite",
        description:
          "Axinite single-page preview shell with localisation and feature flags.",
        theme_color: "#0b1116",
        background_color: "#0b1116",
        display: "standalone",
        lang: "en-GB",
        scope: deployBasePath,
        start_url: `${deployBasePath}chat`,
        icons: [
          {
            src: `${deployBasePath}assets/icons/axinite32.ico`,
            sizes: "32x32",
            type: "image/x-icon",
          },
        ],
      },
    }),
  ],
  build: {
    outDir: resolve(__dirname, "dist"),
    emptyOutDir: true,
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
