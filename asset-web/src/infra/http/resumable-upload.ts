import type { Resource } from "@/domain/resource";
import type {
  UploadDraft,
  UploadProgress,
  UploadPublication,
  UploadReceipt,
} from "@/shared/api/upload";
import type { BlobSha256, FileSha256 } from "../crypto/file-sha256";
import { httpError } from "./http-error";

const UPLOAD_CHUNK_BYTES = 8 * 1024 * 1024;
const UPLOAD_CHUNK_CHECKSUM_ATTEMPTS = 3;
const UPLOAD_RESUME_STORAGE_KEY = "asset-hub.upload-sessions";

interface ApiUploadSession {
  id: string;
  offset: number;
  size: number;
  status: "uploading" | "finalizing" | "completed" | "failed";
  resource_id?: string;
  error?: string;
}

export class ResumableUpload {
  constructor(
    private readonly baseUrl: string,
    private readonly hashFile: FileSha256,
    private readonly hashChunk: BlobSha256,
    private readonly findResource: (id: string, signal?: AbortSignal) => Promise<Resource>,
    private readonly resolveDirectoryId: (path: string) => Promise<string>,
  ) {}

  async upload(
    draft: UploadDraft,
    onProgress?: (progress: UploadProgress) => void,
  ): Promise<UploadReceipt> {
    const file = draft.file;
    reportUploadProgress(onProgress, "preparing", 0, file.size);
    const expectedSha256 = await this.hashFile(file, (bytesHashed) =>
      reportUploadProgress(onProgress, "preparing", bytesHashed, file.size),
    );
    const metadata = {
      name: draft.name.length > 0 ? draft.name : file.name,
      directory_id: await this.resolveDirectoryId(draft.directory),
      mime_type: file.type || "application/octet-stream",
      size: file.size,
      expected_sha256: expectedSha256,
    };
    const fingerprint = uploadFingerprint(file, metadata, expectedSha256);
    let uploadId = loadUploadId(fingerprint);
    let offset = 0;

    if (uploadId) {
      const response = await fetch(`${this.baseUrl}/uploads/${encodeURIComponent(uploadId)}`);
      if (response.ok) {
        const session = parseUploadSession(await response.json());
        offset = session.offset;
        if (offset > file.size || session.size !== file.size) {
          clearUploadId(fingerprint);
          uploadId = null;
          offset = 0;
        }
      } else if (response.status === 404) {
        clearUploadId(fingerprint);
        uploadId = null;
      } else {
        throw await httpError(response);
      }
    }

    if (!uploadId) {
      const response = await fetch(`${this.baseUrl}/uploads`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(metadata),
      });
      if (!response.ok) throw await httpError(response);
      const session = parseUploadSession(await response.json());
      uploadId = session.id;
      offset = session.offset;
      saveUploadId(fingerprint, uploadId);
    }

    reportUploadProgress(onProgress, "uploading", offset, file.size);
    while (offset < file.size) {
      const chunk = file.slice(offset, Math.min(offset + UPLOAD_CHUNK_BYTES, file.size));
      const chunkChecksum = await this.hashChunk(chunk);
      let response: Response | undefined;
      for (let attempt = 1; attempt <= UPLOAD_CHUNK_CHECKSUM_ATTEMPTS; attempt += 1) {
        response = await fetch(`${this.baseUrl}/uploads/${encodeURIComponent(uploadId)}`, {
          method: "PATCH",
          headers: {
            "Content-Type": "application/octet-stream",
            "Upload-Offset": String(offset),
            "Upload-Checksum": chunkChecksum,
          },
          body: chunk,
        });
        if (response.ok) break;
        const error = await httpError(response);
        const retryChecksumMismatch =
          response.status === 409 &&
          error.message.includes("upload chunk checksum mismatch") &&
          attempt < UPLOAD_CHUNK_CHECKSUM_ATTEMPTS;
        if (!retryChecksumMismatch) throw error;
      }
      if (!response?.ok) throw new Error("Upload chunk did not complete");
      const nextOffset = uploadOffset(response);
      if (nextOffset <= offset || nextOffset > offset + chunk.size)
        throw new Error("Upload server did not advance the file offset");
      offset = nextOffset;
      reportUploadProgress(onProgress, "uploading", offset, file.size);
    }

    reportUploadProgress(onProgress, "finalizing", offset, file.size);
    const response = await fetch(
      `${this.baseUrl}/uploads/${encodeURIComponent(uploadId)}/complete`,
      { method: "POST" },
    );
    if (!response.ok) throw await httpError(response);
    parseUploadSession(await response.json());
    return { id: uploadId, name: metadata.name };
  }

  pendingUploads(): UploadReceipt[] {
    return Object.entries(uploadSessions()).flatMap(([fingerprint, id]) => {
      try {
        const value: unknown = JSON.parse(fingerprint);
        if (!value || typeof value !== "object" || !("resource" in value)) return [];
        const metadata = value.resource;
        if (
          !metadata ||
          typeof metadata !== "object" ||
          !("name" in metadata) ||
          typeof metadata.name !== "string"
        )
          return [];
        return [{ id, name: metadata.name }];
      } catch {
        return [];
      }
    });
  }

  async status(id: string, signal?: AbortSignal): Promise<UploadPublication> {
    const response = await fetch(
      `${this.baseUrl}/uploads/${encodeURIComponent(id)}`,
      signal ? { signal } : {},
    );
    if (response.status === 404) {
      return {
        status: "failed",
        message: "Upload session is unavailable. Choose the file again to restart.",
      };
    }
    if (!response.ok) throw await httpError(response);
    const session = parseUploadSession(await response.json());
    if (session.status === "failed") {
      return { status: "failed", message: session.error || "Resource publishing failed" };
    }
    if (session.status === "completed") {
      if (!session.resource_id) throw new Error("Completed upload did not include a Resource ID");
      return {
        status: "completed",
        resource: await this.findResource(session.resource_id, signal),
      };
    }
    return { status: session.status };
  }

  async acknowledge(id: string): Promise<void> {
    // Only called after the published Resource has reached the Query cache.
    clearUploadIdById(id);
    try {
      await fetch(`${this.baseUrl}/uploads/${encodeURIComponent(id)}`, { method: "DELETE" });
    } catch {
      // Acknowledgement cleanup cannot undo successful publication.
    }
  }
}

function uploadOffset(response: Response): number {
  const value = response.headers.get("upload-offset");
  const offset = value === null ? Number.NaN : Number(value);
  if (!Number.isSafeInteger(offset) || offset < 0) {
    throw new Error("Upload server returned an invalid Upload-Offset");
  }
  return offset;
}

function parseUploadSession(value: unknown): ApiUploadSession {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Upload server returned an invalid session");
  }
  const session = value as Record<string, unknown>;
  const statuses: ApiUploadSession["status"][] = ["uploading", "finalizing", "completed", "failed"];
  if (
    typeof session.id !== "string" ||
    !Number.isSafeInteger(session.offset) ||
    Number(session.offset) < 0 ||
    !Number.isSafeInteger(session.size) ||
    Number(session.size) < 0 ||
    !statuses.includes(session.status as ApiUploadSession["status"]) ||
    (session.resource_id !== undefined && typeof session.resource_id !== "string") ||
    (session.error !== undefined && typeof session.error !== "string")
  ) {
    throw new Error("Upload server returned an invalid session");
  }
  return session as unknown as ApiUploadSession;
}

function reportUploadProgress(
  callback: ((progress: UploadProgress) => void) | undefined,
  stage: UploadProgress["stage"],
  bytesSent: number,
  totalBytes: number,
): void {
  callback?.({ stage, bytesSent, totalBytes });
}

function uploadFingerprint(file: File, metadata: object, expectedSha256: string): string {
  return JSON.stringify({
    file: {
      name: file.name,
      size: file.size,
      lastModified: file.lastModified,
      sha256: expectedSha256,
    },
    resource: metadata,
  });
}

function loadUploadId(fingerprint: string): string | null {
  return uploadSessions()[fingerprint] ?? null;
}

function saveUploadId(fingerprint: string, id: string): void {
  const sessions = uploadSessions();
  sessions[fingerprint] = id;
  saveUploadSessions(sessions);
}

function clearUploadId(fingerprint: string): void {
  const sessions = uploadSessions();
  delete sessions[fingerprint];
  saveUploadSessions(sessions);
}

function clearUploadIdById(id: string): void {
  saveUploadSessions(
    Object.fromEntries(Object.entries(uploadSessions()).filter(([, uploadId]) => uploadId !== id)),
  );
}

function uploadSessions(): Record<string, string> {
  try {
    const value = globalThis.localStorage?.getItem(UPLOAD_RESUME_STORAGE_KEY);
    if (!value) return {};
    const parsed: unknown = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(
        (entry): entry is [string, string] => typeof entry[1] === "string",
      ),
    );
  } catch {
    return {};
  }
}

function saveUploadSessions(sessions: Record<string, string>): void {
  try {
    globalThis.localStorage?.setItem(UPLOAD_RESUME_STORAGE_KEY, JSON.stringify(sessions));
  } catch {
    // 浏览器禁用持久化时仍允许当前上传继续。
  }
}
