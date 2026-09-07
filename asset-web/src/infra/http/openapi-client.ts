import createClient from "openapi-fetch";
import type { paths } from "./generated";
import { responseError } from "./http-error";

export function createOpenApiClient(baseUrl: string) {
  return createClient<paths>({ baseUrl });
}

export type OpenApiClient = ReturnType<typeof createOpenApiClient>;

type FetchResult<T> = { data?: T; error?: unknown; response: Response };

export function expectData<T>(result: FetchResult<T>): T {
  if (result.data !== undefined) return result.data;
  throw responseError(result.response, result.error);
}

export function expectSuccess(result: FetchResult<unknown>): void {
  if (result.response.ok) return;
  throw responseError(result.response, result.error);
}
