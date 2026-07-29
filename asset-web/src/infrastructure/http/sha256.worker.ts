import { IncrementalSha256 } from "./incremental-sha256";

type WorkerRequest = { type: "update"; chunk: ArrayBuffer } | { type: "digest" };

const sha256 = new IncrementalSha256();

self.onmessage = (event: MessageEvent<WorkerRequest>) => {
  try {
    if (event.data.type === "update") {
      sha256.update(new Uint8Array(event.data.chunk));
      self.postMessage({ type: "updated" });
      return;
    }
    self.postMessage({ type: "digest", value: sha256.digestHex() });
  } catch (error) {
    self.postMessage({
      type: "error",
      message: error instanceof Error ? error.message : "SHA-256 worker failed",
    });
  }
};
