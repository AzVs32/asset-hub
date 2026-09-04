import { QueryClient } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "@/app/app";
import { AppProviders } from "@/app/providers";
import { createOpenApiGateways } from "@/infra/http/openapi-gateways";
import "./styles.css";

// 创建后端 API Gateway
const gateways = createOpenApiGateways();

// 创建 React Query 客户端：统一管理请求和缓存
const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: 1, refetchOnWindowFocus: false },
    mutations: { retry: false },
  },
});

// 将 React 应用挂在到页面的 root 节点
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {/* 为整个 App 提供全局能力 */}
    <AppProviders gateways={gateways} queryClient={queryClient}>
      {/* 进入应用路由 */}
      <App />
    </AppProviders>
  </React.StrictMode>,
);
