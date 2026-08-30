import React from "react";
import type {
  AppGateways,
  AssetWorkspaceGateway,
  AuthGateway,
  PluginHostGateway,
  UserAdministrationGateway,
} from "./gateways";

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

export function useAuthGateway(): AuthGateway {
  return useGateways().auth;
}

export function useAssetWorkspaceGateway(): AssetWorkspaceGateway {
  return useGateways().assetWorkspace;
}

export function usePluginHostGateway(): PluginHostGateway {
  return useGateways().pluginHost;
}

export function useUserAdministrationGateway(): UserAdministrationGateway {
  return useGateways().userAdministration;
}

function useGateways(): AppGateways {
  const gateways = React.useContext(GatewayContext);
  if (!gateways) throw new Error("GatewayProvider is missing");
  return gateways;
}
