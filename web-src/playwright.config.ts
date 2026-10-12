/**
 * @file Playwright configuration for the browser tests in `axinite/tests/e2e`.
 * It starts the local `dev` runner (mock backend plus preview)
 * on its owned ports, enables the restart fixture, and tests Chromium only.
 */

import { defineConfig, devices } from "@playwright/test";
import { parsePort } from "./mock-backend/src/ports";

process.env["PLAYWRIGHT_BROWSERS_PATH"] ??= "0";

// The coordinator chooses its owned server pair once. Workers inherit these
// values when Playwright reloads this config, rather than choosing ports from
// their own PIDs. Busy ports fail startup without reusing another server.
// Keep defaults below the host's ephemeral range: outgoing connections can
// occupy high ports even when no listening server appears in a port check.
const candidateApiPort = 20_000 + (process.pid % 5_000) * 2;
const apiPort = parsePort(
  "MOCK_API_PORT",
  process.env["MOCK_API_PORT"],
  candidateApiPort
);
const baseURL =
  process.env["PLAYWRIGHT_BASE_URL"] ??
  `http://127.0.0.1:${candidateApiPort + 1}`;
process.env["MOCK_API_PORT"] = String(apiPort);
process.env["PLAYWRIGHT_BASE_URL"] = baseURL;
const previewURL = new URL(baseURL);
const previewPort =
  previewURL.port || (previewURL.protocol === "https:" ? "443" : "80");

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
    reuseExistingServer: false,
    env: {
      MOCK_API_PORT: String(apiPort),
      PREVIEW_PORT: previewPort,
      MOCK_RESTART_FIXTURE: "1",
    },
    stdout: "pipe",
    stderr: "pipe",
    timeout: 180_000,
  },
});
