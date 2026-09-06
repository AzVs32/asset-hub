import type { AppGateways } from "@/shared/api/gateways";
import {
  type BlobSha256,
  calculateBlobSha256,
  calculateFileSha256,
  type FileSha256,
} from "./file-sha256";
import { OpenApiAssetWorkspaceGateway } from "./openapi-asset-workspace-gateway";
import { createOpenApiClient } from "./openapi-client";

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
    assetWorkspace: assetGateway,
  };
}
