import { defineConfig, devices } from "@playwright/test";

process.env.PLAYWRIGHT_BROWSERS_PATH ??= "0";

const baseURL = process.env.PLAYWRIGHT_BASE_URL ?? "http://127.0.0.1:2020";

export default defineConfig({
  testDir: "./axinite/tests/e2e",
  testMatch: ["**/*.pw.ts"],
  timeout: 60_000,
  expect: {
    timeout: 10_000,
  },
  use: {
    baseURL,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
      },
    },
  ],
  webServer: {
    command: "bun run dev",
    url: baseURL,
    reuseExistingServer: true,
    stdout: "pipe",
    stderr: "pipe",
    timeout: 180_000,
  },
});
