import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Preview } from "./components/Preview";
import { NetworkHealth } from "./components/NetworkHealth";
import { EncodeSettings } from "./components/EncodeSettings";

export default function App() {
  const [isStreaming, setIsStreaming] = useState(false);
  const [streamId, setStreamId] = useState<string | null>(null);
  const [preset, setPreset] = useState("high");
  const [hwAccel, setHwAccel] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handleGoLive() {
    setError(null);
    try {
      const id = await invoke<string>("start_stream", {
        title: "Prism Stream",
        qualityPreset: preset,
      });
      setStreamId(id);
      setIsStreaming(true);
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleStop() {
    setError(null);
    try {
      await invoke("stop_stream");
      setStreamId(null);
      setIsStreaming(false);
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "1fr 320px",
        gap: 16,
        padding: 16,
        minHeight: "100vh",
        background: "#171923",
        boxSizing: "border-box",
      }}
    >
      {/* Left column: preview */}
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <Preview isStreaming={isStreaming} />
        {streamId && (
          <div
            style={{
              fontSize: 12,
              color: "#718096",
              fontFamily: "monospace",
              wordBreak: "break-all",
            }}
          >
            Stream ID: {streamId}
          </div>
        )}
        {error && (
          <div style={{ color: "#fc8181", fontSize: 13, padding: "8px 12px", background: "#2d2020", borderRadius: 6 }}>
            {error}
          </div>
        )}
      </div>

      {/* Right column: controls */}
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <NetworkHealth
          isStreaming={isStreaming}
          onGoLive={handleGoLive}
          onStop={handleStop}
        />
        <EncodeSettings
          preset={preset}
          hwAccel={hwAccel}
          onPresetChange={setPreset}
          onHwAccelChange={setHwAccel}
          disabled={isStreaming}
        />
      </div>
    </div>
  );
}
