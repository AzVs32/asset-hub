export class HttpError extends Error {
  readonly status: number;
  readonly code: string | null;
  readonly details: unknown;

  constructor(message: string, status: number, code: string | null = null, details?: unknown) {
    super(message);
    this.name = "HttpError";
    this.status = status;
    this.code = code;
    this.details = details;
  }
}

export async function httpError(response: Response, payload?: unknown): Promise<HttpError> {
  const body = payload ?? (await parseBody(response));
  if (body && typeof body === "object" && "error" in body) {
    const document = body as { error?: unknown; code?: unknown; details?: unknown };
    return new HttpError(
      typeof document.error === "string" ? document.error : response.statusText,
      response.status,
      typeof document.code === "string" ? document.code : null,
      document.details,
    );
  }
  return new HttpError(response.statusText || `HTTP ${response.status}`, response.status);
}

async function parseBody(response: Response): Promise<unknown> {
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("application/json")) return undefined;
  return response.json().catch(() => undefined);
}
