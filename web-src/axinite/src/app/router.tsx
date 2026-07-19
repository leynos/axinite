import {
  createRootRoute,
  createRoute,
  createRouter,
  Navigate,
  Outlet,
  RouterProvider,
} from "@tanstack/solid-router";

import { AppShell } from "@/components/app-shell";
import { RoutePage } from "@/components/route-page";
import { normaliseBasePath } from "@/lib/base-path";

const routerBasePath = normaliseBasePath(
  import.meta.env.BASE_URL as string | undefined
);

const NotFoundRedirect = () => {
  const invalidPath =
    typeof window === "undefined" ? "(unknown path)" : window.location.pathname;
  console.warn(
    `[router] Redirecting unmatched route "${invalidPath}" to /chat`
  );
  return <Navigate to="/chat" />;
};

const rootRoute = createRootRoute({
  component: () => (
    <AppShell>
      <Outlet />
    </AppShell>
  ),
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: () => <Navigate to="/chat" />,
});

const chatRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/chat",
  component: () => <RoutePage routeId="chat" />,
});

const memoryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/memory",
  component: () => <RoutePage routeId="memory" />,
});

const jobsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/jobs",
  component: () => <RoutePage routeId="jobs" />,
});

const routinesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/routines",
  component: () => <RoutePage routeId="routines" />,
});

const extensionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/extensions",
  component: () => <RoutePage routeId="extensions" />,
});

const skillsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/skills",
  component: () => <RoutePage routeId="skills" />,
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  chatRoute,
  memoryRoute,
  jobsRoute,
  routinesRoute,
  extensionsRoute,
  skillsRoute,
]);

const router = createRouter({
  routeTree,
  basepath: routerBasePath,
  defaultPreload: "intent",
  defaultNotFoundComponent: NotFoundRedirect,
});

declare module "@tanstack/solid-router" {
  interface Register {
    router: typeof router;
  }
}

export const AppRouter = () => <RouterProvider router={router} />;
