declare module "i18next-fluent-backend" {
  import type { BackendModule, Services } from "i18next";

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
    init?(
      options?: FluentBackendOptions,
      callback?: (error?: unknown) => void
    ): void;
  }

  export default FluentBackend;
}
