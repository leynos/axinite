/**
 * @file Behaviour tests for the memory route (`MemoryPreview`) using the real
 * API module against controlled in-process gateway responses.
 */

import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { AppProviders } from "@/app/providers";
import { MemoryPreview } from "@/components/memory-preview";
import { isJsonObject, isString } from "@/lib/api/json-value";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { GatewayTestServer } from "./support/gateway-test-server";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

let gateway: GatewayTestServer;

beforeAll(async () => {
  await setupI18nTestHarness();
});

beforeEach(async () => {
  gateway = new GatewayTestServer()
    .respond("GET", "/api/memory/tree", {
      entries: [
        { path: "notes/alpha.md", is_dir: false },
        { path: "notes/beta.md", is_dir: false },
      ],
    })
    .route("GET", "/api/memory/read", async (request) => {
      const path = new URL(request.url).searchParams.get("path") ?? "";
      return Response.json({
        path,
        content: `Body of ${path}`,
        updated_at: null,
      });
    })
    .route("POST", "/api/memory/write", async (request) => {
      const body: unknown = await request.json();
      if (!isJsonObject(body) || !isString(body["path"])) {
        return new Response("Invalid write request", { status: 400 });
      }
      return Response.json({ path: body["path"] });
    });
  globalThis.localStorage.clear();
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);
});

describe("memory preview editing", () => {
  it("locks document selection while a draft is open", async () => {
    render(() => (
      <AppProviders>
        <MemoryPreview />
      </AppProviders>
    ));

    const beta = await screen.findByRole("button", { name: "beta.md" });
    await screen.findByText("Body of notes/alpha.md");
    expect(beta).toBeEnabled();

    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(beta).toBeDisabled();

    // Switching is impossible, so Save writes the draft to the open document.
    await userEvent.click(beta);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(
        gateway.requests.some(
          (request) =>
            request.method === "POST" &&
            request.path === "/api/memory/write" &&
            request.body?.["path"] === "notes/alpha.md"
        )
      ).toBe(true);
    });

    await waitFor(() => {
      expect(beta).toBeEnabled();
    });
  });
});
