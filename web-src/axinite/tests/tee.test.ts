import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  checkTeeStatus,
  fetchTeeReport,
  resetTeeReportCache,
  teeApiBase,
  teeInstanceName,
} from "@/lib/tee";

const originalLocation = window.location;

function setLocation(href: string): void {
  Object.defineProperty(window, "location", {
    configurable: true,
    value: new URL(href),
  });
}

beforeEach(() => {
  resetTeeReportCache();
});

afterEach(() => {
  Object.defineProperty(window, "location", {
    configurable: true,
    value: originalLocation,
  });
  vi.restoreAllMocks();
});

describe("teeApiBase", () => {
  it("derives the sibling api host from a multi-label domain", () => {
    expect(teeApiBase("sub.example.com")).toBe(
      `${window.location.protocol}//api.example.com`
    );
  });

  it.each([
    ["localhost", "localhost"],
    ["loopback v4", "127.0.0.1"],
    ["loopback v6", "::1"],
    ["bare ipv4", "203.0.113.5"],
    ["single label", "myhost"],
  ])("is inert for %s", (_name, hostname) => {
    expect(teeApiBase(hostname)).toBeNull();
  });

  it("uses the current protocol", () => {
    expect(teeApiBase("node.http.test")).toMatch(/^https?:\/\/api\./u);
  });
});

describe("teeInstanceName", () => {
  it("is the first hostname label", () => {
    expect(teeInstanceName("instance-7.example.com")).toBe("instance-7");
  });
});

describe("checkTeeStatus", () => {
  it("returns null on localhost without fetching", async () => {
    setLocation("http://localhost:3000/");
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    await expect(checkTeeStatus()).resolves.toBeNull();
    expect(fetchSpy).not.toHaveBeenCalled();
  });

  it("fetches the instance attestation and requires image_digest", async () => {
    setLocation("https://sub.example.com/");
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ image_digest: "sha256:abc" }), {
        status: 200,
      })
    );

    await expect(checkTeeStatus()).resolves.toEqual({
      image_digest: "sha256:abc",
    });
    expect(fetchSpy).toHaveBeenCalledWith(
      "https://api.example.com/instances/sub/attestation"
    );
  });

  it("rejects when image_digest is absent", async () => {
    setLocation("https://sub.example.com/");
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({}), { status: 200 })
    );
    await expect(checkTeeStatus()).rejects.toThrow(/image_digest/u);
  });
});

describe("fetchTeeReport", () => {
  it("fetches the report once and caches it", async () => {
    setLocation("https://sub.example.com/");
    const payload = {
      tls_certificate_fingerprint: "AA:BB",
      report_data: "deadbeef",
      vm_config: "sev-snp",
    };
    const fetchSpy = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify(payload), { status: 200 })
      );

    await expect(fetchTeeReport()).resolves.toEqual(payload);
    await expect(fetchTeeReport()).resolves.toEqual(payload);
    expect(fetchSpy).toHaveBeenCalledTimes(1);
    expect(fetchSpy).toHaveBeenCalledWith(
      "https://api.example.com/attestation/report"
    );
  });
});
