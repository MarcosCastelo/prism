import { useState, useEffect, useCallback } from "react";
import { openDB, IDBPDatabase } from "idb";
import * as nobleEd from "@noble/ed25519";
import { sha512 } from "@noble/hashes/sha512";

// noble/ed25519 v2 requires sha512 for both sync and async operations.
nobleEd.etc.sha512Sync = (...m) => sha512(nobleEd.etc.concatBytes(...m));
nobleEd.etc.sha512Async = async (...m) => sha512(nobleEd.etc.concatBytes(...m));

export interface Identity {
  publicKeyHex: string;
  displayName: string;
  sign: (data: Uint8Array) => Promise<Uint8Array>;
}

interface StoredIdentity {
  publicKeyHex: string;
  privateKeyBytes: Uint8Array;
  displayName: string;
  createdAt: number;
  encrypted: false;
}

const DB_NAME = "prism-identity";
const STORE_NAME = "identity";
const DB_VERSION = 1;

async function openIdentityDb(): Promise<IDBPDatabase> {
  return openDB(DB_NAME, DB_VERSION, {
    upgrade(db) {
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME, { keyPath: "publicKeyHex" });
      }
    },
  });
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.slice(i, i + 2), 16);
  }
  return bytes;
}

function makeIdentity(stored: StoredIdentity): Identity {
  return {
    publicKeyHex: stored.publicKeyHex,
    displayName: stored.displayName,
    sign: async (data: Uint8Array) => {
      return nobleEd.sign(data, stored.privateKeyBytes);
    },
  };
}

export function useIdentity(): {
  identity: Identity | null;
  isLoading: boolean;
  create: (displayName: string, password?: string) => Promise<Identity>;
  load: () => Promise<Identity | null>;
  export: () => Promise<Blob>;
} {
  const [identity, setIdentity] = useState<Identity | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // Auto-load on mount
  useEffect(() => {
    load().finally(() => setIsLoading(false));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const create = useCallback(
    async (displayName: string, _password?: string): Promise<Identity> => {
      // Note: password-based encryption (AES-GCM + PBKDF2) is not yet implemented.
      // The optional `password` parameter is accepted for API compatibility with the PRD.
      const privateKeyBytes = nobleEd.utils.randomPrivateKey();
      const publicKeyBytes = await nobleEd.getPublicKey(privateKeyBytes);
      const publicKeyHex = bytesToHex(publicKeyBytes);

      const stored: StoredIdentity = {
        publicKeyHex,
        privateKeyBytes,
        displayName: displayName.slice(0, 32),
        createdAt: Date.now(),
        encrypted: false,
      };

      const db = await openIdentityDb();
      await db.put(STORE_NAME, stored);

      const id = makeIdentity(stored);
      setIdentity(id);
      return id;
    },
    []
  );

  const load = useCallback(async (): Promise<Identity | null> => {
    const db = await openIdentityDb();
    const all = await db.getAll(STORE_NAME);
    if (all.length === 0) return null;
    const stored = (all as StoredIdentity[]).sort(
      (a, b) => b.createdAt - a.createdAt
    )[0];
    const id = makeIdentity(stored);
    setIdentity(id);
    return id;
  }, []);

  const exportFn = useCallback(async (): Promise<Blob> => {
    const db = await openIdentityDb();
    const all = await db.getAll(STORE_NAME);
    if (all.length === 0) throw new Error("No identity to export");
    const stored = (all as StoredIdentity[]).sort(
      (a, b) => b.createdAt - a.createdAt
    )[0];
    const exportData = {
      publicKeyHex: stored.publicKeyHex,
      privateKeyHex: bytesToHex(stored.privateKeyBytes),
      displayName: stored.displayName,
      createdAt: stored.createdAt,
    };
    return new Blob([JSON.stringify(exportData, null, 2)], {
      type: "application/json",
    });
  }, []);

  return {
    identity,
    isLoading,
    create,
    load,
    export: exportFn,
  };
}
