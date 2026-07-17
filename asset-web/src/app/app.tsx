import { RouterProvider } from "react-router";
import { AuthBoundary } from "@/features/auth/auth-boundary";
import { router } from "./router";

export function App() {
  return (
    <AuthBoundary>
      <RouterProvider router={router} />
    </AuthBoundary>
  );
}
