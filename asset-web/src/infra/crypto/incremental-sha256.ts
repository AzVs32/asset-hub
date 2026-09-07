const INITIAL_STATE = [
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
] as const;

const ROUND_CONSTANTS = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
] as const;

export class IncrementalSha256 {
  readonly #state = new Uint32Array(INITIAL_STATE);
  readonly #buffer = new Uint8Array(64);
  readonly #schedule = new Uint32Array(64);
  #bufferLength = 0;
  #bytesHashed = 0;
  #finished = false;

  update(data: Uint8Array): void {
    if (this.#finished) throw new Error("SHA-256 digest is already finalized");
    this.#bytesHashed += data.byteLength;
    if (!Number.isSafeInteger(this.#bytesHashed)) {
      throw new Error("File is too large to hash safely");
    }

    let offset = 0;
    if (this.#bufferLength > 0) {
      const length = Math.min(64 - this.#bufferLength, data.byteLength);
      this.#buffer.set(data.subarray(0, length), this.#bufferLength);
      this.#bufferLength += length;
      offset = length;
      if (this.#bufferLength === 64) {
        this.#compress(this.#buffer, 0);
        this.#bufferLength = 0;
      }
    }

    while (offset + 64 <= data.byteLength) {
      this.#compress(data, offset);
      offset += 64;
    }
    if (offset < data.byteLength) {
      const remaining = data.subarray(offset);
      this.#buffer.set(remaining, 0);
      this.#bufferLength = remaining.byteLength;
    }
  }

  digestHex(): string {
    if (this.#finished) throw new Error("SHA-256 digest is already finalized");
    this.#finished = true;

    this.#buffer[this.#bufferLength] = 0x80;
    this.#bufferLength += 1;
    if (this.#bufferLength > 56) {
      this.#buffer.fill(0, this.#bufferLength);
      this.#compress(this.#buffer, 0);
      this.#bufferLength = 0;
    }
    this.#buffer.fill(0, this.#bufferLength, 56);
    const highBits = Math.floor(this.#bytesHashed / 0x20000000);
    const lowBits = (this.#bytesHashed % 0x20000000) * 8;
    const view = new DataView(this.#buffer.buffer);
    view.setUint32(56, highBits, false);
    view.setUint32(60, lowBits, false);
    this.#compress(this.#buffer, 0);

    return Array.from(this.#state, (word) => word.toString(16).padStart(8, "0")).join("");
  }

  #compress(chunk: Uint8Array, offset: number): void {
    const schedule = this.#schedule;
    const view = new DataView(chunk.buffer, chunk.byteOffset + offset, 64);
    for (let index = 0; index < 16; index += 1) {
      schedule[index] = view.getUint32(index * 4, false);
    }
    for (let index = 16; index < 64; index += 1) {
      const first = valueAt(schedule, index - 15);
      const second = valueAt(schedule, index - 2);
      const sigma0 = rotateRight(first, 7) ^ rotateRight(first, 18) ^ (first >>> 3);
      const sigma1 = rotateRight(second, 17) ^ rotateRight(second, 19) ^ (second >>> 10);
      schedule[index] =
        (valueAt(schedule, index - 16) + sigma0 + valueAt(schedule, index - 7) + sigma1) >>> 0;
    }

    let a = valueAt(this.#state, 0);
    let b = valueAt(this.#state, 1);
    let c = valueAt(this.#state, 2);
    let d = valueAt(this.#state, 3);
    let e = valueAt(this.#state, 4);
    let f = valueAt(this.#state, 5);
    let g = valueAt(this.#state, 6);
    let h = valueAt(this.#state, 7);

    for (let index = 0; index < 64; index += 1) {
      const sum1 = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
      const choice = (e & f) ^ (~e & g);
      const temp1 =
        (h + sum1 + choice + valueAt(ROUND_CONSTANTS, index) + valueAt(schedule, index)) >>> 0;
      const sum0 = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (sum0 + majority) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + temp1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) >>> 0;
    }

    this.#state[0] = (valueAt(this.#state, 0) + a) >>> 0;
    this.#state[1] = (valueAt(this.#state, 1) + b) >>> 0;
    this.#state[2] = (valueAt(this.#state, 2) + c) >>> 0;
    this.#state[3] = (valueAt(this.#state, 3) + d) >>> 0;
    this.#state[4] = (valueAt(this.#state, 4) + e) >>> 0;
    this.#state[5] = (valueAt(this.#state, 5) + f) >>> 0;
    this.#state[6] = (valueAt(this.#state, 6) + g) >>> 0;
    this.#state[7] = (valueAt(this.#state, 7) + h) >>> 0;
  }
}

function rotateRight(value: number, bits: number): number {
  return (value >>> bits) | (value << (32 - bits));
}

function valueAt(values: ArrayLike<number>, index: number): number {
  const value = values[index];
  if (value === undefined) throw new Error("SHA-256 internal index is out of bounds");
  return value;
}
