import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import type { ParentComponent } from "solid-js";

import { FeatureFlagProvider } from "@/lib/feature-flags/runtime";
import { I18nProvider } from "@/lib/i18n/provider";

/**
 * Providers wrapper for behaviour tests that exercise TanStack Query.
 *
 * Unlike the app-level `AppProviders`, each instance builds a fresh
 * `QueryClient`, so query caches never leak between tests in the same file.
 */
export const TestProviders: ParentComponent = (props) => {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        staleTime: 0,
        gcTime: 0,
      },
    },
  });

  return (
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <FeatureFlagProvider>{props.children}</FeatureFlagProvider>
      </I18nProvider>
    </QueryClientProvider>
  );
};
