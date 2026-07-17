import { createBrowserRouter, Navigate } from "react-router";

export const router = createBrowserRouter([
  {
    path: "/",
    lazy: async () => ({
      Component: (await import("@/features/resources/resource-workspace")).ResourceWorkspace,
    }),
  },
  {
    path: "/view/:resourceId/:actionId",
    lazy: async () => ({
      Component: (await import("@/features/resources/standalone-plugin-view")).StandalonePluginView,
    }),
  },
  { path: "*", element: <Navigate to="/" replace /> },
]);
