import { createBrowserRouter } from "react-router";
import { AuthBoundary } from "@/features/auth/auth-boundary";
import { LOGIN_PATH } from "@/shared/routing/paths";

export const router = createBrowserRouter([
  {
    Component: AuthBoundary,
    children: [
      { path: LOGIN_PATH, element: null },
      {
        path: "/*",
        lazy: async () => ({
          Component: (await import("@/features/asset-workspace/asset-workspace")).AssetWorkspace,
        }),
      },
    ],
  },
]);
