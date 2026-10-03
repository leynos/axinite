/**
 * Root provider stack for the SPA: TanStack Query cache, i18n, and feature
 * flags. `main.tsx` wraps the auth gate and router in `AppProviders`, so every
 * route can rely on those contexts being present.
 */

import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import type { ParentComponent } from "solid-js";

import { FeatureFlagProvider } from "@/lib/feature-flags/runtime";
import { I18nProvider } from "@/lib/i18n/provider";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      staleTime: 15_000,
    },
  },
});

export const AppProviders: ParentComponent = (props) => {
  return (
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <FeatureFlagProvider>{props.children}</FeatureFlagProvider>
      </I18nProvider>
    </QueryClientProvider>
  );
};
