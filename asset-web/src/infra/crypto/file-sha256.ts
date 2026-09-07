const HASH_CHUNK_BYTES = 8 * 1024 * 1024;

type WorkerResponse =
  | { type: "updated" }
  | { type: "digest"; value: string }
  | { type: "error"; message: string };

export type FileSha256 = (
  file: File,
  onProgress?: (bytesHashed: number) => void,
) => Promise<string>;

export type BlobSha256 = (blob: Blob) => Promise<string>;

export const calculateBlobSha256: BlobSha256 = async (blob) => {
  const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
};

export const calculateFileSha256: FileSha256 = async (file, onProgress) => {
  const worker = new Worker(new URL("./sha256.worker.ts", import.meta.url), { type: "module" });
  try {
    let offset = 0;
    while (offset < file.size) {
      const chunk = await file
        .slice(offset, Math.min(offset + HASH_CHUNK_BYTES, file.size))
        .arrayBuffer();
      const chunkLength = chunk.byteLength;
      await requestWorker(worker, { type: "update", chunk }, [chunk]);
      offset += chunkLength;
      onProgress?.(offset);
    }
    const response = await requestWorker(worker, { type: "digest" });
    if (response.type !== "digest") throw new Error("SHA-256 worker returned no digest");
    return response.value;
  } finally {
    worker.terminate();
  }
};

function requestWorker(
  worker: Worker,
  message: { type: "update"; chunk: ArrayBuffer } | { type: "digest" },
  transfer: Transferable[] = [],
): Promise<WorkerResponse> {
  return new Promise((resolve, reject) => {
    worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
      if (event.data.type === "error") {
        reject(new Error(event.data.message));
      } else {
        resolve(event.data);
      }
    };
    worker.onerror = (event) => reject(new Error(event.message || "SHA-256 worker failed"));
    worker.postMessage(message, transfer);
  });
}
