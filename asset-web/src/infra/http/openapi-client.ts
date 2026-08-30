import createClient from "openapi-fetch";
import type { paths } from "./generated";
import { applicationError, HttpError } from "./http-error";

export function createOpenApiClient(baseUrl: string) {
  return createClient<paths>({ baseUrl, credentials: "include" });
}

export type OpenApiClient = ReturnType<typeof createOpenApiClient>;

type FetchResult<T> = { data?: T; error?: unknown; response: Response };

export function expectData<T>(result: FetchResult<T>): T {
  if (result.data !== undefined) return result.data;
  throw apiResultError(result);
}

export function expectSuccess(result: FetchResult<unknown>): void {
  if (result.response.ok) return;
  throw apiResultError(result);
}

function apiResultError(result: FetchResult<unknown>): Error {
  const error = result.error;
  if (error && typeof error === "object" && "error" in error) {
    const document = error as { error?: unknown; code?: unknown; details?: unknown };
    return applicationError(
      new HttpError(
        typeof document.error === "string" ? document.error : result.response.statusText,
        result.response.status,
        typeof document.code === "string" ? document.code : null,
        document.details,
      ),
    );
  }
  return applicationError(
    new HttpError(
      result.response.statusText || `HTTP ${result.response.status}`,
      result.response.status,
    ),
  );
}
