/**
 * @file Test harness that boots the real i18n runtime without a gateway: it
 * stubs `fetch` to serve locale `.ftl` files from `axinite/public` plus minimal
 * `/api/gateway/status` and `/api/features` replies, and fails on anything else.
 */

import { readFile } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";

import { DEFAULT_LOCALE } from "@/lib/i18n/supported-locales";

const projectRoot = resolve(process.cwd(), "axinite");
const publicDir = resolve(projectRoot, "public");

async function readLocaleAsset(pathname: string): Promise<string> {
  const relativePath = pathname.replace(/^\//u, "");
  const diskPath = resolve(publicDir, relativePath);
  const safeRelativePath = relative(publicDir, diskPath);

  if (safeRelativePath.startsWith("..") || isAbsolute(safeRelativePath)) {
    throw new Error(`Refusing to read locale asset outside ${publicDir}`);
  }

  return await readFile(diskPath, "utf8");
}

/**
 * Install the harness `fetch` stub on `globalThis`. The shared Vitest setup
 * files call this for every test file: any module that imports the i18n
 * runtime starts its backend load at import time, and without the stub that
 * load fails, i18next schedules retries, and a retry timer can fire after the
 * test environment is torn down (`fetch is not defined`).
 */
export function installTestFetch(): void {
  globalThis.fetch = (async (input) => {
    const url =
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url;
    const pathname = url.replace(/^https?:\/\/[^/]+/u, "");

    if (pathname.startsWith("/locales/")) {
      const body = await readLocaleAsset(pathname);
      return new Response(body, {
        status: 200,
        headers: {
          "Content-Type": "text/plain; charset=utf-8",
        },
      });
    }

    if (pathname === "/api/gateway/status") {
      return new Response(
        JSON.stringify({
          connected: false,
          status: "Preview",
        }),
        {
          status: 200,
          headers: {
            "Content-Type": "application/json",
          },
        }
      );
    }

    if (pathname === "/api/features") {
      return new Response(JSON.stringify({ flags: {} }), {
        status: 200,
        headers: {
          "Content-Type": "application/json",
        },
      });
    }

    throw new Error(`Unhandled test fetch: ${pathname}`);
  }) as typeof globalThis.fetch;
}

/** Boot the real i18n runtime against the stubbed gateway. */
export async function setupI18nTestHarness(): Promise<void> {
  installTestFetch();
  window.localStorage.setItem("i18nextLng", DEFAULT_LOCALE);
  const { i18nReady } = await import("@/lib/i18n/runtime");
  await i18nReady;
}
