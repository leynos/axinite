import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

import { AppProviders } from "@/app/providers";
import { AuthGate } from "@/components/auth-gate";
import { clearGatewayToken, getGatewayToken } from "@/lib/auth/token";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

beforeAll(async () => {
  await setupI18nTestHarness();
});

afterEach(() => {
  clearGatewayToken();
  vi.unstubAllGlobals();
});

function jsonResponse(status: number, body: unknown = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("auth gate behaviour", () => {
  it("renders children when the gateway accepts anonymous access", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
        jsonResponse(200)
      )
    );

    render(() => (
      <AppProviders>
        <AuthGate>
          <div>Protected content</div>
        </AuthGate>
      </AppProviders>
    ));

    await waitFor(() => {
      expect(screen.getByText("Protected content")).toBeVisible();
    });
  });

  it("prompts for a token when the gateway returns 401 and unlocks on success", async () => {
    let acceptedToken: string | null = null;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
        const headers = new Headers(init?.headers);
        const auth = headers.get("Authorization");
        if (auth === "Bearer valid-token") {
          acceptedToken = auth;
          return jsonResponse(200);
        }
        return jsonResponse(401, { error: "unauthorized" });
      })
    );

    render(() => (
      <AppProviders>
        <AuthGate>
          <div>Protected content</div>
        </AuthGate>
      </AppProviders>
    ));

    const input = await screen.findByLabelText("Access token");
    await userEvent.type(input, "valid-token");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() => {
      expect(screen.getByText("Protected content")).toBeVisible();
    });
    expect(acceptedToken).toBe("Bearer valid-token");
    expect(getGatewayToken()).toBe("valid-token");
  });

  it("shows a rejection message for a bad token and keeps the gate closed", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
        const headers = new Headers(init?.headers);
        if (headers.get("Authorization") === "Bearer good") {
          return jsonResponse(200);
        }
        return jsonResponse(401, { error: "unauthorized" });
      })
    );

    render(() => (
      <AppProviders>
        <AuthGate>
          <div>Protected content</div>
        </AuthGate>
      </AppProviders>
    ));

    const input = await screen.findByLabelText("Access token");
    await userEvent.type(input, "wrong-token");
    await userEvent.click(screen.getByRole("button", { name: "Connect" }));

    await waitFor(() => {
      expect(screen.getByText("The gateway rejected the token.")).toBeVisible();
    });
    expect(screen.queryByText("Protected content")).toBeNull();
    expect(getGatewayToken()).toBeNull();
  });
});
