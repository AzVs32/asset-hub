import { QueryClient } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "@/app/app";
import { AppProviders } from "@/app/providers";
import { OpenApiAssetGateway } from "@/infrastructure/http/openapi-asset-gateway";
import { PluginKernel } from "@/kernel/plugin-kernel";
import { registerDefaultViewRenderers } from "@/plugins/renderers/default-renderers";
import "./styles.css";

const gateway = new OpenApiAssetGateway();
const kernel = new PluginKernel();
registerDefaultViewRenderers(kernel);
const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
    mutations: { retry: false },
  },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppProviders gateway={gateway} kernel={kernel} queryClient={queryClient}>
      <App />
    </AppProviders>
  </React.StrictMode>,
);
