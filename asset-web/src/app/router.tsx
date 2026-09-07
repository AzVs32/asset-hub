import { createBrowserRouter } from "react-router";

export const router = createBrowserRouter([
  {
    path: "/*",
    lazy: async () => ({
      Component: (await import("@/features/asset-workspace/asset-workspace")).AssetWorkspace,
    }),
  },
]);
