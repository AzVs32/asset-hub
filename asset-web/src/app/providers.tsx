import CssBaseline from "@mui/material/CssBaseline";
import { ThemeProvider } from "@mui/material/styles";
import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type React from "react";
import { Toaster } from "sonner";
import { type PluginKernel, PluginKernelProvider } from "@/kernel/plugin-kernel";
import { GatewayProvider } from "@/shared/api/gateway-context";
import type { AppGateways } from "@/shared/api/gateways";
import { theme } from "@/theme";

export function AppProviders({
  gateways,
  kernel,
  queryClient,
  children,
}: {
  gateways: AppGateways;
  kernel: PluginKernel;
  queryClient: QueryClient;
  children: React.ReactNode;
}) {
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateways={gateways}>
          <PluginKernelProvider kernel={kernel}>
            {children}
            <Toaster richColors position="bottom-right" />
          </PluginKernelProvider>
        </GatewayProvider>
      </QueryClientProvider>
    </ThemeProvider>
  );
}
