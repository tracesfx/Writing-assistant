/**
 * The provenance record's text hash (SPEC §7.6). The app hashes synchronously
 * in plain TypeScript; this checks it against the platform's SHA-256.
 */
import { describe, expect, it } from "vitest";

import { sha256Hex } from "./provenance";

async function reference(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

describe("sha256", () => {
  it("matches the standard test vectors", () => {
    expect(sha256Hex("")).toBe("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    expect(sha256Hex("abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    expect(sha256Hex("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")).toBe(
      "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );
  });

  it("agrees with the platform on UTF-8, block edges and long text", async () => {
    const samples = [
      "é",
      "naïve café — “quoted”… 日本語 🙂",
      "a".repeat(55),
      "a".repeat(56),
      "a".repeat(63),
      "a".repeat(64),
      "a".repeat(65),
      "a".repeat(1000),
      "One two.\n\nThree four five.\n\n".repeat(500),
    ];
    for (const t of samples) expect(sha256Hex(t), t.slice(0, 20)).toBe(await reference(t));
  });
});
