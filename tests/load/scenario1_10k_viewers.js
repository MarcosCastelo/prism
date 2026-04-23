/**
 * Scenario 1 — 10,000 simultaneous viewers
 *
 * PRD pass criteria:
 *   - Latency P95 < 20 s (streamer → viewer)
 *   - Frame loss < 0.5% under stable conditions
 *   - No node CPU > 90% for more than 30 s
 *   - New viewer receives first HLS segment in < 10 s
 *
 * Usage (against a running testnet):
 *   EDGE_URL=http://<edge-node>:8080 STREAM_ID=<hex> \
 *   k6 run --vus 10000 --duration 300s tests/load/scenario1_10k_viewers.js
 *
 * Dry-run (CI syntax check only):
 *   k6 run --dry-run tests/load/scenario1_10k_viewers.js
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

// ── Configuration ────────────────────────────────────────────────────────────

const EDGE_URL   = __ENV.EDGE_URL   || "http://localhost:8080";
const STREAM_ID  = __ENV.STREAM_ID  || "aaaaaaaaaaaaaaaa";
const SEGMENT_MS = 3000; // HLS segment duration

// ── Custom metrics ───────────────────────────────────────────────────────────

const timeToFirstSegment = new Trend("time_to_first_segment_ms", true);
const segmentFetchTime   = new Trend("segment_fetch_time_ms", true);
const frameLoss          = new Rate("frame_loss_rate");
const joinFailures       = new Counter("join_failures");

// ── Test options ─────────────────────────────────────────────────────────────

export const options = {
  stages: [
    // Ramp up to 10,000 viewers over 2 minutes
    { duration: "2m",  target: 10000 },
    // Hold at 10,000 for 3 minutes
    { duration: "3m",  target: 10000 },
    // Ramp down
    { duration: "1m",  target: 0 },
  ],
  thresholds: {
    // PRD: P95 latency (first segment) < 20s
    time_to_first_segment_ms: ["p(95)<20000"],
    // PRD: frame loss < 0.5%
    frame_loss_rate: ["rate<0.005"],
    // Segment fetches succeed > 99.5%
    "http_req_failed{type:segment}": ["rate<0.005"],
    // P95 HTTP response time < 5s (per-segment)
    "http_req_duration{type:segment}": ["p(95)<5000"],
  },
};

// ── Helpers ──────────────────────────────────────────────────────────────────

function manifestUrl() {
  return `${EDGE_URL}/stream/${STREAM_ID}/index.m3u8`;
}

function segmentUrl(segmentPath) {
  // segmentPath may be absolute or relative to the manifest
  if (segmentPath.startsWith("http")) return segmentPath;
  return `${EDGE_URL}/stream/${STREAM_ID}/${segmentPath}`;
}

/**
 * Parse an HLS manifest and return the list of .ts / .m4s segment URLs
 * from the highest-bandwidth rendition.
 */
function parseManifest(body, baseUrl) {
  const lines = body.split("\n").map((l) => l.trim());

  // If this is a master playlist, follow the first rendition URI.
  const renditionLine = lines.find(
    (l) => !l.startsWith("#") && (l.endsWith(".m3u8") || l.includes(".m3u8"))
  );
  if (renditionLine) {
    const renditionUrl = renditionLine.startsWith("http")
      ? renditionLine
      : `${baseUrl}/${renditionLine}`;
    const res = http.get(renditionUrl, { tags: { type: "manifest" } });
    if (res.status !== 200) return [];
    return parseManifest(res.body, renditionUrl.replace(/\/[^/]+$/, ""));
  }

  // Media playlist: collect segment paths
  return lines.filter(
    (l) => !l.startsWith("#") && l.length > 0 && (l.includes(".ts") || l.includes(".m4s"))
  );
}

// ── VU main ──────────────────────────────────────────────────────────────────

export default function () {
  const joinStart = Date.now();

  // Fetch HLS manifest
  const manifestRes = http.get(manifestUrl(), { tags: { type: "manifest" } });
  if (!check(manifestRes, { "manifest 200": (r) => r.status === 200 })) {
    joinFailures.add(1);
    sleep(5);
    return;
  }

  const baseUrl = `${EDGE_URL}/stream/${STREAM_ID}`;
  const segments = parseManifest(manifestRes.body, baseUrl);
  if (segments.length === 0) {
    joinFailures.add(1);
    sleep(5);
    return;
  }

  // Fetch the first segment — measure join latency
  const firstSegStart = Date.now();
  const firstRes = http.get(segmentUrl(segments[0]), { tags: { type: "segment" } });
  const firstOk = check(firstRes, { "first segment 200": (r) => r.status === 200 });

  if (firstOk) {
    timeToFirstSegment.add(Date.now() - joinStart);
  } else {
    joinFailures.add(1);
    frameLoss.add(true);
    sleep(3);
    return;
  }

  // Simulate continuous playback: fetch subsequent segments at real-time pace
  for (let i = 1; i < Math.min(segments.length, 10); i++) {
    sleep(SEGMENT_MS / 1000); // wait one segment duration between fetches

    const segStart = Date.now();
    const res = http.get(segmentUrl(segments[i]), { tags: { type: "segment" } });
    segmentFetchTime.add(Date.now() - segStart);

    const ok = check(res, { "segment 200": (r) => r.status === 200 });
    frameLoss.add(!ok); // true = loss
  }
}
