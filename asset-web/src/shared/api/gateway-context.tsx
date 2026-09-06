import React from "react";
import type { AppGateways, AssetWorkspaceGateway } from "./gateways";

const GatewayContext = React.createContext<AppGateways | null>(null);

export function GatewayProvider({
  gateways,
  children,
}: {
  gateways: AppGateways;
  children: React.ReactNode;
}) {
  return <GatewayContext.Provider value={gateways}>{children}</GatewayContext.Provider>;
}

export function useAssetWorkspaceGateway(): AssetWorkspaceGateway {
  return useGateways().assetWorkspace;
}

function useGateways(): AppGateways {
  const gateways = React.useContext(GatewayContext);
  if (!gateways) throw new Error("GatewayProvider is missing");
  return gateways;
}
