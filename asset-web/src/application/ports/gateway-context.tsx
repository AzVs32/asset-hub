import React from "react";
import type { AssetGateway } from "./asset-gateway";

const GatewayContext = React.createContext<AssetGateway | null>(null);

export function GatewayProvider({
  gateway,
  children,
}: {
  gateway: AssetGateway;
  children: React.ReactNode;
}) {
  return <GatewayContext.Provider value={gateway}>{children}</GatewayContext.Provider>;
}

export function useGateway(): AssetGateway {
  const gateway = React.useContext(GatewayContext);
  if (!gateway) throw new Error("GatewayProvider is missing");
  return gateway;
}
