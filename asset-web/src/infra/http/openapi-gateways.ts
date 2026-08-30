import type { AppGateways } from "@/shared/api/gateways";
import {
  type BlobSha256,
  calculateBlobSha256,
  calculateFileSha256,
  type FileSha256,
} from "./file-sha256";
import { OpenApiAssetWorkspaceGateway } from "./openapi-asset-workspace-gateway";
import { OpenApiAuthGateway } from "./openapi-auth-gateway";
import { createOpenApiClient } from "./openapi-client";
import { OpenApiUserAdministrationGateway } from "./openapi-user-administration-gateway";

export function createOpenApiGateways(
  baseUrl = import.meta.env.VITE_API_BASE_URL || "/api",
  hashFile: FileSha256 = calculateFileSha256,
  hashChunk: BlobSha256 = calculateBlobSha256,
): AppGateways {
  const normalizedBaseUrl = baseUrl.replace(/\/$/, "");
  const client = createOpenApiClient(normalizedBaseUrl);
  const assetGateway = new OpenApiAssetWorkspaceGateway(
    client,
    normalizedBaseUrl,
    hashFile,
    hashChunk,
  );

  return {
    auth: new OpenApiAuthGateway(client),
    assetWorkspace: assetGateway,
    pluginHost: assetGateway,
    userAdministration: new OpenApiUserAdministrationGateway(client),
  };
}
