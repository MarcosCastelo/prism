import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

interface StreamMetrics {
  active_nodes: number;
  viewer_count: number;
  bitrate_kbps: number;
  latency_ms: number;
  uptime_s: number;
}

interface NetworkHealthProps {
  isStreaming: boolean;
  onGoLive: () => Promise<void>;
  onStop: () => Promise<void>;
}

export function NetworkHealth({ isStreaming, onGoLive, onStop }: NetworkHealthProps) {
  const [metrics, setMetrics] = useState<StreamMetrics>({
    active_nodes: 0,
    viewer_count: 0,
    bitrate_kbps: 0,
    latency_ms: 0,
    uptime_s: 0,
  });
  const [loading, setLoading] = useState(false);

  // Listen for periodic metrics-update events from the Tauri backend.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;

    listen<StreamMetrics>("metrics-update", (event) => {
      setMetrics(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  function formatUptime(s: number): string {
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const sec = s % 60;
    return [h, m, sec].map((v) => String(v).padStart(2, "0")).join(":");
  }

  async function handleGoLive() {
    setLoading(true);
    try {
      await onGoLive();
    } finally {
      setLoading(false);
    }
  }

  async function handleStop() {
    setLoading(true);
    try {
      await onStop();
    } finally {
      setLoading(false);
    }
  }

  return (
    <div style={{ padding: 16, background: "#1a202c", borderRadius: 8, color: "#e2e8f0" }}>
      <h3 style={{ margin: "0 0 12px", fontSize: 14, textTransform: "uppercase", color: "#a0aec0" }}>
        Network Health
      </h3>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, marginBottom: 16 }}>
        <Stat label="Nodes" value={String(metrics.active_nodes)} />
        <Stat label="Viewers" value={String(metrics.viewer_count)} />
        <Stat label="Bitrate" value={`${(metrics.bitrate_kbps / 1000).toFixed(1)} Mbps`} />
        <Stat label="Latency" value={`${(metrics.latency_ms / 1000).toFixed(0)} s`} />
        {isStreaming && <Stat label="Uptime" value={formatUptime(metrics.uptime_s)} />}
      </div>

      <div style={{ display: "flex", gap: 8 }}>
        {!isStreaming ? (
          <button
            onClick={handleGoLive}
            disabled={loading}
            style={{
              flex: 1,
              padding: "10px 0",
              background: "#e53e3e",
              color: "#fff",
              border: "none",
              borderRadius: 6,
              fontWeight: 700,
              cursor: loading ? "not-allowed" : "pointer",
            }}
          >
            {loading ? "Starting…" : "GO LIVE"}
          </button>
        ) : (
          <button
            onClick={handleStop}
            disabled={loading}
            style={{
              flex: 1,
              padding: "10px 0",
              background: "#4a5568",
              color: "#fff",
              border: "none",
              borderRadius: 6,
              fontWeight: 700,
              cursor: loading ? "not-allowed" : "pointer",
            }}
          >
            {loading ? "Stopping…" : "ENCERRAR"}
          </button>
        )}
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div style={{ fontSize: 11, color: "#718096", marginBottom: 2 }}>{label}</div>
      <div style={{ fontSize: 18, fontWeight: 600 }}>{value}</div>
    </div>
  );
}
