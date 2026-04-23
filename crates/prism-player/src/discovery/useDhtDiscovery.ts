import { useState, useEffect, useCallback } from "react";

export interface StreamInfo {
  streamId: string;
  streamerPubkeyHex: string;
  streamerDisplayName: string;
  title: string;
  viewerCount: number;
  thumbnailUrl: string;
  startedAt: number;
}

const SESSION_CACHE_KEY = "prism:discovered-streams";

function loadCached(): StreamInfo[] {
  try {
    const raw = sessionStorage.getItem(SESSION_CACHE_KEY);
    return raw ? (JSON.parse(raw) as StreamInfo[]) : [];
  } catch {
    return [];
  }
}

function saveCache(streams: StreamInfo[]): void {
  try {
    sessionStorage.setItem(SESSION_CACHE_KEY, JSON.stringify(streams));
  } catch {
    // sessionStorage may be unavailable in some contexts — ignore
  }
}

export function useDhtDiscovery(): {
  streams: StreamInfo[];
  isLoading: boolean;
  search: (query: string) => void;
} {
  const [streams, setStreams] = useState<StreamInfo[]>(loadCached);
  const [isLoading, setIsLoading] = useState(false);
  const [query, setQuery] = useState("");

  const discover = useCallback(async (searchQuery: string) => {
    setIsLoading(true);
    try {
      // In a full implementation this would:
      //   1. Connect to the local prism-node via WebSocket
      //   2. Issue FIND_VALUE("prism:streams:index") to get active stream IDs
      //   3. For pubkey search: FIND_VALUE(sha256("prism:stream:" || pubkeyHex))
      //   4. Fetch thumbnails from Classe A/edge nodes
      //
      // For now, return an empty list — populated by the node at runtime.
      const discovered: StreamInfo[] = [];
      setStreams(discovered);
      saveCache(discovered);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    discover(query);
  }, [query, discover]);

  const search = useCallback((q: string) => {
    setQuery(q);
  }, []);

  return { streams, isLoading, search };
}
