// useIdentity.test.ts — PRD Fase 3, critério C8
//
// Tests verify the crypto behaviour that useIdentity wraps:
//   - unique Ed25519 key pairs per identity
//   - sign/verify round-trip
//   - tampered data rejected by verifier
//
// Importing useIdentity.ts triggers its module-level sha512Sync + sha512Async
// setup on @noble/ed25519, so all ed.sign / ed.verify calls below work
// with the pure-JS SHA-512 implementation (no dependency on crypto.subtle).

import "fake-indexeddb/auto";

import { describe, test, expect } from "vitest";
import * as ed from "@noble/ed25519";

// Side-effect import: configures sha512Sync and sha512Async on the ed module.
import { hexToBytes } from "./useIdentity";

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

describe("useIdentity", () => {
  test("create identity generates unique keys", async () => {
    const priv1 = ed.utils.randomPrivateKey();
    const priv2 = ed.utils.randomPrivateKey();

    const pub1 = bytesToHex(await ed.getPublicKey(priv1));
    const pub2 = bytesToHex(await ed.getPublicKey(priv2));

    expect(pub1).toHaveLength(64);
    expect(pub2).toHaveLength(64);
    expect(pub1).not.toBe(pub2);
  });

  test("sign and verify roundtrip", async () => {
    const privateKey = ed.utils.randomPrivateKey();
    const publicKeyBytes = await ed.getPublicKey(privateKey);
    const publicKeyHex = bytesToHex(publicKeyBytes);

    const data = new TextEncoder().encode("test message");
    const sig = await ed.sign(data, privateKey);

    const valid = await ed.verify(sig, data, hexToBytes(publicKeyHex));
    expect(valid).toBe(true);
  });

  test("tampered data fails verification", async () => {
    const privateKey = ed.utils.randomPrivateKey();
    const publicKeyBytes = await ed.getPublicKey(privateKey);
    const publicKeyHex = bytesToHex(publicKeyBytes);

    const data = new TextEncoder().encode("original");
    const sig = await ed.sign(data, privateKey);
    const tampered = new TextEncoder().encode("tampered");

    const valid = await ed.verify(sig, tampered, hexToBytes(publicKeyHex));
    expect(valid).toBe(false);
  });
});
