import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type React from "react";
import { Toaster } from "sonner";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import { type PluginKernel, PluginKernelProvider } from "@/kernel/plugin-kernel";

export function AppProviders({
  gateway,
  kernel,
  queryClient,
  children,
}: {
  gateway: AssetGateway;
  kernel: PluginKernel;
  queryClient: QueryClient;
  children: React.ReactNode;
}) {
  return (
    <QueryClientProvider client={queryClient}>
      <GatewayProvider gateway={gateway}>
        <PluginKernelProvider kernel={kernel}>
          {children}
          <Toaster richColors position="bottom-right" />
        </PluginKernelProvider>
      </GatewayProvider>
    </QueryClientProvider>
  );
}
