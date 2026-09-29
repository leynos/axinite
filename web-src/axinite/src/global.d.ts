// Inline `import(...)` type keeps this file an ambient script (no top-level
// import), so the `declare module` below stays a global ambient declaration
// while `interface Window` merges into the global Window type.
interface Window {
  // Deliberate e2e test-hook surface; see src/lib/test-hooks.ts.
  __axinite?: import("@/lib/test-hooks").AxiniteTestHooks;
}

declare module "i18next-fluent-backend" {
  import type {
    BackendModule,
    InitOptions,
    ReadCallback,
    Services,
  } from "i18next";

  interface FluentBackendOptions {
    loadPath?: string;
    ajax?: (
      url: string,
      options: Record<string, unknown>,
      callback: (
        data: string | Error,
        xhr: { status: number; statusText?: string }
      ) => void
    ) => void;
  }

  class FluentBackend implements BackendModule<FluentBackendOptions> {
    static type: "backend";
    constructor(services?: Services, options?: FluentBackendOptions);
    init(
      services: Services,
      backendOptions: FluentBackendOptions,
      i18nextOptions: InitOptions
    ): void;
    read(language: string, namespace: string, callback: ReadCallback): void;
  }

  export default FluentBackend;
}
