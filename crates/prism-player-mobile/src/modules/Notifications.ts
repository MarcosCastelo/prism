// Push notifications without a central server.
//
// Design: poll the DHT every 60s for each followed streamer's stream record.
// If a stream record appears that wasn't there before, fire a local
// notification via Expo Notifications — no Firebase, no APNs server push.
//
// All state is kept in-process; the app must be foregrounded or in background
// (not terminated) for polling to run. Background fetch is handled by the
// Expo background task API in a future iteration.

import * as Notifications from "expo-notifications";

Notifications.setNotificationHandler({
  handleNotification: async () => ({
    shouldShowAlert: true,
    shouldPlaySound: true,
    shouldSetBadge: false,
  }),
});

// Edge node base URL — configurable via environment or app config.
const EDGE_BASE_URL = process.env.EXPO_PUBLIC_EDGE_URL ?? "http://localhost:8080";

export async function requestNotificationPermission(): Promise<boolean> {
  const { status } = await Notifications.requestPermissionsAsync();
  return status === "granted";
}

export async function scheduleLocalNotification(
  streamerId: string,
  streamerName: string,
  streamTitle: string
): Promise<void> {
  await Notifications.scheduleNotificationAsync({
    content: {
      title: `${streamerName} está ao vivo!`,
      body: streamTitle,
      data: { streamerId },
    },
    trigger: null, // fire immediately
  });
}

interface StreamRecord {
  stream_id: string;
  title: string;
  started_at_ms: number;
}

async function fetchStreamRecord(
  pubkeyHex: string
): Promise<StreamRecord | null> {
  try {
    const url = `${EDGE_BASE_URL}/stream/${pubkeyHex}/latest`;
    const res = await fetch(url, { signal: AbortSignal.timeout(5_000) });
    if (!res.ok) return null;
    return (await res.json()) as StreamRecord;
  } catch {
    return null;
  }
}

const notifiedStreamIds = new Set<string>();

async function checkAndNotify(pubkeyHex: string): Promise<void> {
  const record = await fetchStreamRecord(pubkeyHex);
  if (!record) return;
  if (notifiedStreamIds.has(record.stream_id)) return;

  notifiedStreamIds.add(record.stream_id);
  // Use truncated pubkey as display name until identity resolution is available.
  const displayName = `pr1${pubkeyHex.slice(0, 4)}...${pubkeyHex.slice(-4)}`;
  await scheduleLocalNotification(pubkeyHex, displayName, record.title);
}

let pollingHandle: ReturnType<typeof setInterval> | null = null;

export async function startFollowPolling(
  followedStreamers: string[], // array of pubkey_hex
  intervalMs: number = 60_000
): Promise<void> {
  if (pollingHandle !== null) {
    stopFollowPolling();
  }

  for (const pubkeyHex of followedStreamers) {
    await checkAndNotify(pubkeyHex);
  }

  pollingHandle = setInterval(async () => {
    for (const pubkeyHex of followedStreamers) {
      await checkAndNotify(pubkeyHex);
    }
  }, intervalMs);
}

export function stopFollowPolling(): void {
  if (pollingHandle !== null) {
    clearInterval(pollingHandle);
    pollingHandle = null;
  }
}
