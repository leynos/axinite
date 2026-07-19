import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import { TestProviders } from "./support/test-providers";
import { TeeAttestation } from "@/components/tee-attestation";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { resetTeeReportCache } from "@/lib/tee";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";

const STATUS = { image_digest: "sha256:cafef00d" };
const REPORT = {
  tls_certificate_fingerprint: "AA:BB:CC:DD",
  report_data: "0123456789abcdef0123456789abcdefEXTRA-TRUNCATED-TAIL",
  vm_config: "sev-snp/v4",
};

const originalLocation = window.location;
let harnessFetch: typeof globalThis.fetch;

function setLocation(href: string): void {
  Object.defineProperty(window, "location", {
    configurable: true,
    value: new URL(href),
  });
}

function enableFlag(): void {
  window.localStorage.setItem(
    "axinite.feature-flag-overrides",
    JSON.stringify({ surface_tee_attestation: true })
  );
}

beforeAll(async () => {
  await setupI18nTestHarness();
  harnessFetch = globalThis.fetch;
});

beforeEach(async () => {
  window.localStorage.clear();
  resetTeeReportCache();
  document.documentElement.lang = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input.toString();
    if (url.includes("/instances/") && url.endsWith("/attestation")) {
      return Promise.resolve(
        new Response(JSON.stringify(STATUS), { status: 200 })
      );
    }
    if (url.endsWith("/attestation/report")) {
      return Promise.resolve(
        new Response(JSON.stringify(REPORT), { status: 200 })
      );
    }
    return harnessFetch(input, init);
  }) as typeof globalThis.fetch;
});

afterEach(() => {
  globalThis.fetch = harnessFetch;
  Object.defineProperty(window, "location", {
    configurable: true,
    value: originalLocation,
  });
  vi.restoreAllMocks();
});

describe("TEE attestation surface", () => {
  it("stays inert when the feature flag is off", async () => {
    setLocation("https://sub.example.com/");

    render(() => (
      <TestProviders>
        <TeeAttestation />
      </TestProviders>
    ));

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(
      screen.queryByRole("button", { name: "View TEE attestation" })
    ).toBeNull();
  });

  it("stays inert on localhost even with the flag on", async () => {
    setLocation("http://localhost:3000/");
    enableFlag();

    render(() => (
      <TestProviders>
        <TeeAttestation />
      </TestProviders>
    ));

    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(
      screen.queryByRole("button", { name: "View TEE attestation" })
    ).toBeNull();
  });

  it("renders attestation fields and copies the merged report", async () => {
    setLocation("https://sub.example.com/");
    enableFlag();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(() => (
      <TestProviders>
        <TeeAttestation />
      </TestProviders>
    ));

    const shield = await screen.findByRole("button", {
      name: "View TEE attestation",
    });
    await userEvent.click(shield);

    expect(await screen.findByText("Image digest")).toBeVisible();
    expect(screen.getByText("sha256:cafef00d")).toBeVisible();
    expect(screen.getByText("AA:BB:CC:DD")).toBeVisible();
    // Report data is truncated to 32 characters plus an ellipsis.
    expect(screen.getByText("0123456789abcdef0123456789abcdef…")).toBeVisible();

    await userEvent.click(
      screen.getByRole("button", { name: "Copy full report" })
    );

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledTimes(1);
    });
    const payload = writeText.mock.calls[0][0] as string;
    expect(payload).toContain("sha256:cafef00d");
    expect(payload).toContain("AA:BB:CC:DD");
  });
});
