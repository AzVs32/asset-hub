import { createBrowserRouter } from "react-router";
import { AuthBoundary } from "@/features/auth/auth-boundary";

export const router = createBrowserRouter([
  {
    Component: AuthBoundary,
    children: [
      { path: "/login", element: null },
      {
        path: "/view/:resourceId/:actionId",
        lazy: async () => ({
          Component: (await import("@/features/asset-workspace/standalone-resource-plugin-view"))
            .StandaloneResourcePluginView,
        }),
      },
      {
        path: "/*",
        lazy: async () => ({
          Component: (await import("@/features/asset-workspace/asset-workspace")).AssetWorkspace,
        }),
      },
    ],
  },
]);
