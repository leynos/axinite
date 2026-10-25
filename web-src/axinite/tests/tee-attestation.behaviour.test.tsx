/**
 * @file Behaviour tests for the `TeeAttestation` component. The browser location
 * is overridden to simulate hosted domains, since attestation is inert on
 * localhost and other hosts without a sibling API host.
 */

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
import { TeeAttestation } from "@/components/tee-attestation";
import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";
import { resetTeeReportCache } from "@/lib/tee";
import { setupI18nTestHarness } from "./support/i18n-test-runtime";
import { TestProviders } from "./support/test-providers";

const STATUS = { image_digest: "sha256:cafef00d" };
const REPORT = {
  tls_certificate_fingerprint: "AA:BB:CC:DD",
  report_data: "0123456789abcdef0123456789abcdefEXTRA-TRUNCATED-TAIL",
  vm_config: "sev-snp/v4",
};

const originalLocation = globalThis.location;
let harnessFetch: typeof globalThis.fetch;

function setLocation(href: string): void {
  Object.defineProperty(globalThis, "location", {
    configurable: true,
    value: new URL(href),
  });
}

function enableFlag(): void {
  globalThis.localStorage.setItem(
    "axinite.feature-flag-overrides",
    JSON.stringify({ surface_tee_attestation: true })
  );
}

beforeAll(async () => {
  await setupI18nTestHarness();
  harnessFetch = globalThis.fetch;
});

beforeEach(async () => {
  globalThis.localStorage.clear();
  resetTeeReportCache();
  document.documentElement.lang = "";
  const runtime = await import("@/lib/i18n/runtime");
  await runtime.default.changeLanguage(DEFAULT_LOCALE);

  const attestationFetch = Object.assign(
    (...args: Parameters<typeof globalThis.fetch>) => {
      const [input, init] = args;
      const url = input instanceof Request ? input.url : input.toString();
      if (url.includes("/instances/") && url.endsWith("/attestation")) {
        return Promise.resolve(Response.json(STATUS, { status: 200 }));
      }
      if (url.endsWith("/attestation/report")) {
        return Promise.resolve(Response.json(REPORT, { status: 200 }));
      }
      return harnessFetch(input, init);
    },
    { preconnect: () => {} }
  );
  globalThis.fetch = attestationFetch;
});

afterEach(() => {
  globalThis.fetch = harnessFetch;
  Object.defineProperty(globalThis, "location", {
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
    const writeText = vi
      .spyOn(globalThis.navigator.clipboard, "writeText")
      .mockImplementation(() => Promise.resolve());

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
    const payload = writeText.mock.calls[0]?.[0];
    expect(payload).toContain("sha256:cafef00d");
    expect(payload).toContain("AA:BB:CC:DD");
  });
});
