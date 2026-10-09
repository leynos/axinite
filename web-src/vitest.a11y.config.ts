import { resolve } from "node:path";

import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [solid()],
  test: {
    name: "accessibility",
    environment: "happy-dom",
    globals: true,
    setupFiles: ["./axinite/tests/setup-vitest-a11y.ts"],
    include: [
      "axinite/tests/**/*.a11y.test.ts",
      "axinite/tests/**/*.a11y.test.tsx",
    ],
  },
  resolve: {
    alias: {
      "@": resolve(__dirname, "axinite/src"),
    },
  },
});
