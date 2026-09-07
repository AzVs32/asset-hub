import { createBrowserRouter } from "react-router";
import { RouteError, RouteLoading } from "./route-error";

export const router = createBrowserRouter([
  {
    path: "/*",
    ErrorBoundary: RouteError,
    HydrateFallback: RouteLoading,
    lazy: async () => ({
      Component: (await import("@/features/asset-workspace/asset-workspace")).AssetWorkspace,
    }),
  },
]);
