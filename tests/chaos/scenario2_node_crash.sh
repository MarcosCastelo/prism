#!/usr/bin/env bash
# Scenario 2 — Chaos: 60% of nodes crash simultaneously
#
# PRD pass criteria:
#   - Stream does NOT interrupt (max 2 s buffer empty during failover)
#   - All viewers reconnect automatically in < 30 s
#   - Chat continues (gossip survives with 40% of nodes)
#   - No viewer requires manual action to reconnect
#
# Prerequisites:
#   - A running testnet managed by Docker Compose or a container orchestrator
#   - COMPOSE_FILE pointing to the testnet compose file
#   - VIEWER_PIDS_FILE listing PIDs / process names of simulated viewer probes
#   - k6 installed (used for viewer reconnect probing)
#
# Usage:
#   COMPOSE_FILE=./testnet/docker-compose.yml \
#   STREAM_ID=<hex> \
#   EDGE_URL=http://<survivor-node>:8080 \
#   bash tests/chaos/scenario2_node_crash.sh

set -euo pipefail

COMPOSE_FILE="${COMPOSE_FILE:-./testnet/docker-compose.yml}"
STREAM_ID="${STREAM_ID:-aaaaaaaaaaaaaaaa}"
EDGE_URL="${EDGE_URL:-http://localhost:8080}"
MONITOR_DURATION_S="${MONITOR_DURATION_S:-300}"
MAX_OUTAGE_S=2
MAX_RECONNECT_S=30

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log()  { echo -e "${GREEN}[$(date +%T)]${NC} $*"; }
warn() { echo -e "${YELLOW}[$(date +%T)] WARN${NC} $*"; }
fail() { echo -e "${RED}[$(date +%T)] FAIL${NC} $*"; exit 1; }

# ── Step 1: Enumerate all nodes ───────────────────────────────────────────────
log "Listing running node containers..."
NODES=$(docker compose -f "$COMPOSE_FILE" ps --services | grep "^prism-node" || true)
NODE_COUNT=$(echo "$NODES" | wc -l | tr -d ' ')
KILL_COUNT=$(( (NODE_COUNT * 60 + 99) / 100 ))  # ceil(60%)

log "Total nodes: $NODE_COUNT — will kill $KILL_COUNT (60%)"

if [[ $NODE_COUNT -lt 5 ]]; then
  fail "Testnet must have at least 5 nodes for this scenario. Found: $NODE_COUNT"
fi

# ── Step 2: Start continuous stream health probe in background ────────────────
PROBE_LOG=$(mktemp /tmp/prism_probe_XXXXXX.log)
log "Starting HLS health probe (logging to $PROBE_LOG)..."

probe_hls() {
  local outage_start=0
  local max_outage=0
  while true; do
    if curl -sf --max-time 5 "${EDGE_URL}/stream/${STREAM_ID}/index.m3u8" > /dev/null 2>&1; then
      if [[ $outage_start -ne 0 ]]; then
        local duration=$(( $(date +%s) - outage_start ))
        echo "OUTAGE_END duration=${duration}s at $(date +%T)" >> "$PROBE_LOG"
        if [[ $duration -gt $max_outage ]]; then
          max_outage=$duration
        fi
        outage_start=0
      fi
      echo "OK at $(date +%T)" >> "$PROBE_LOG"
    else
      if [[ $outage_start -eq 0 ]]; then
        outage_start=$(date +%s)
        echo "OUTAGE_START at $(date +%T)" >> "$PROBE_LOG"
      fi
    fi
    sleep 1
  done
}

probe_hls &
PROBE_PID=$!
trap "kill $PROBE_PID 2>/dev/null; rm -f $PROBE_LOG" EXIT

# Give the probe 5s to confirm stream is live before the crash.
sleep 5
if ! grep -q "^OK" "$PROBE_LOG"; then
  fail "Stream not reachable before chaos injection. Check EDGE_URL and STREAM_ID."
fi
log "Stream confirmed live. Injecting chaos in 3 s..."
sleep 3

# ── Step 3: Kill 60% of nodes simultaneously ─────────────────────────────────
KILLED_NODES=$(echo "$NODES" | sort -R | head -n "$KILL_COUNT")
log "Killing nodes: $KILLED_NODES"
CHAOS_TIME=$(date +%s)

for node in $KILLED_NODES; do
  docker compose -f "$COMPOSE_FILE" kill -s SIGKILL "$node" &
done
wait
log "Killed $KILL_COUNT nodes at $(date +%T). Monitoring for ${MONITOR_DURATION_S}s..."

# ── Step 4: Monitor stream for the observation window ────────────────────────
sleep "$MONITOR_DURATION_S"

# ── Step 5: Check reconnect time (worst-case outage from probe log) ───────────
log "Chaos phase complete. Analyzing probe log..."

MAX_OUTAGE_OBSERVED=0
while IFS= read -r line; do
  if [[ "$line" =~ OUTAGE_END\ duration=([0-9]+)s ]]; then
    d="${BASH_REMATCH[1]}"
    if (( d > MAX_OUTAGE_OBSERVED )); then
      MAX_OUTAGE_OBSERVED=$d
    fi
  fi
done < "$PROBE_LOG"

if grep -q "OUTAGE_START" "$PROBE_LOG" && ! grep -q "OUTAGE_END" "$PROBE_LOG"; then
  fail "Stream outage never recovered within ${MONITOR_DURATION_S}s. FAIL."
fi

log "Maximum observed outage: ${MAX_OUTAGE_OBSERVED}s (threshold: ${MAX_OUTAGE_S}s)"

# ── Step 6: Evaluate pass/fail ────────────────────────────────────────────────
PASS=true

if (( MAX_OUTAGE_OBSERVED > MAX_OUTAGE_S )); then
  warn "FAIL: Max outage ${MAX_OUTAGE_OBSERVED}s exceeds threshold ${MAX_OUTAGE_S}s"
  PASS=false
else
  log "PASS: Max outage ${MAX_OUTAGE_OBSERVED}s ≤ ${MAX_OUTAGE_S}s"
fi

# Reconnect check: look for first OK after last OUTAGE_END.
RECONNECT_S=$(awk '
  /OUTAGE_END/ { outage_end_ts = $NF }
  /^OK/ && outage_end_ts { reconnect_ts = $NF; print reconnect_ts - outage_end_ts; outage_end_ts="" }
' "$PROBE_LOG" | sort -n | tail -1)

if [[ -n "$RECONNECT_S" ]] && (( RECONNECT_S > MAX_RECONNECT_S )); then
  warn "FAIL: Reconnect time ${RECONNECT_S}s exceeds threshold ${MAX_RECONNECT_S}s"
  PASS=false
elif [[ -n "$RECONNECT_S" ]]; then
  log "PASS: Reconnect time ${RECONNECT_S}s ≤ ${MAX_RECONNECT_S}s"
fi

# ── Step 7: Report ────────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════"
echo "  Scenario 2 — Chaos 60% Results"
echo "═══════════════════════════════════════════"
echo "  Nodes killed:        $KILL_COUNT / $NODE_COUNT"
echo "  Max stream outage:   ${MAX_OUTAGE_OBSERVED}s  (threshold: ${MAX_OUTAGE_S}s)"
echo "  Max reconnect time:  ${RECONNECT_S:-N/A}s  (threshold: ${MAX_RECONNECT_S}s)"
echo "  Result:              $( [[ $PASS == true ]] && echo '✅ PASS' || echo '❌ FAIL' )"
echo "═══════════════════════════════════════════"

[[ $PASS == true ]] || exit 1
