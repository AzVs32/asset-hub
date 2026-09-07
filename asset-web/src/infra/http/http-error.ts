import { ConcurrentModificationError } from "@/shared/api/errors";

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

export async function httpError(response: Response, payload?: unknown): Promise<Error> {
  return responseError(response, payload ?? (await parseBody(response)));
}

export function responseError(response: Response, body: unknown): Error {
  if (body && typeof body === "object" && "error" in body) {
    const document = body as { error?: unknown; code?: unknown; details?: unknown };
    return applicationError(
      new HttpError(
        typeof document.error === "string" ? document.error : response.statusText,
        response.status,
        typeof document.code === "string" ? document.code : null,
        document.details,
      ),
    );
  }
  return applicationError(
    new HttpError(response.statusText || `HTTP ${response.status}`, response.status),
  );
}

function applicationError(error: HttpError): Error {
  if (error.code === "concurrency.revision_conflict") {
    return new ConcurrentModificationError();
  }
  return error;
}

async function parseBody(response: Response): Promise<unknown> {
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("application/json")) return undefined;
  return response.json().catch(() => undefined);
}
