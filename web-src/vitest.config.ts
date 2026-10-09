import { resolve } from "node:path";

import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [solid()],
  test: {
    name: "unit-and-behaviour",
    environment: "happy-dom",
    globals: true,
    setupFiles: ["./axinite/tests/setup-vitest.ts"],
    include: ["axinite/tests/**/*.test.ts", "axinite/tests/**/*.test.tsx"],
    exclude: [
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
