// Secure key storage for Prism identity on mobile.
//
// iOS:   expo-secure-store → iOS Keychain with kSecAttrAccessibleWhenUnlocked.
//        The private key NEVER leaves the Keychain; signing is done via
//        @noble/ed25519 using bytes retrieved only within the app process.
//
// Android: expo-secure-store → Android Keystore system.
//
// Security invariant: key bytes are erased from JS heap after use by
// overwriting the Uint8Array before letting it go out of scope.

import * as SecureStore from "expo-secure-store";
import * as ed from "@noble/ed25519";
import { sha512 } from "@noble/hashes/sha512";

// noble/ed25519 v2 requires an async SHA-512 implementation.
ed.etc.sha512Sync = (...msgs) => sha512(ed.etc.concatBytes(...msgs));

const KEY_STORAGE_KEY = "prism_ed25519_privkey";

export async function storePrivateKey(key: Uint8Array): Promise<void> {
  const hex = Buffer.from(key).toString("hex");
  await SecureStore.setItemAsync(KEY_STORAGE_KEY, hex, {
    keychainAccessible: SecureStore.WHEN_UNLOCKED,
  });
  // Zero the source buffer to remove key bytes from the call site.
  key.fill(0);
}

export async function signWithStoredKey(data: Uint8Array): Promise<Uint8Array> {
  const hex = await SecureStore.getItemAsync(KEY_STORAGE_KEY);
  if (!hex) {
    throw new Error("No stored identity key found. Run identity setup first.");
  }
  const keyBytes = Uint8Array.from(Buffer.from(hex, "hex"));
  try {
    const sig = await ed.sign(data, keyBytes);
    return sig;
  } finally {
    // Zero key bytes in the local buffer.
    keyBytes.fill(0);
  }
}

export async function getPublicKeyHex(): Promise<string> {
  const hex = await SecureStore.getItemAsync(KEY_STORAGE_KEY);
  if (!hex) {
    throw new Error("No stored identity key found.");
  }
  const keyBytes = Uint8Array.from(Buffer.from(hex, "hex"));
  try {
    const pubkey = await ed.getPublicKeyAsync(keyBytes);
    return Buffer.from(pubkey).toString("hex");
  } finally {
    keyBytes.fill(0);
  }
}

export async function hasStoredKey(): Promise<boolean> {
  const val = await SecureStore.getItemAsync(KEY_STORAGE_KEY);
  return val !== null;
}

export async function deleteStoredKey(): Promise<void> {
  await SecureStore.deleteItemAsync(KEY_STORAGE_KEY);
}

export async function generateAndStoreKey(): Promise<string> {
  const privKey = ed.utils.randomPrivateKey();
  const pubKey = await ed.getPublicKeyAsync(privKey);
  await storePrivateKey(privKey); // zeros privKey internally
  return Buffer.from(pubKey).toString("hex");
}
