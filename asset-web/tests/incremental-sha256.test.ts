import { describe, expect, it } from "vitest";
import { IncrementalSha256 } from "@/infrastructure/http/incremental-sha256";

const encoder = new TextEncoder();

describe("IncrementalSha256", () => {
  it("matches standard SHA-256 vectors", () => {
    const empty = new IncrementalSha256();
    expect(empty.digestHex()).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );

    const abc = new IncrementalSha256();
    abc.update(encoder.encode("abc"));
    expect(abc.digestHex()).toBe(
      "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
  });

  it("produces the same digest across arbitrary chunk boundaries", () => {
    const digest = new IncrementalSha256();
    digest.update(encoder.encode("hello"));
    digest.update(encoder.encode(" "));
    digest.update(encoder.encode("world"));

    expect(digest.digestHex()).toBe(
      "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
    );
  });
});
