import { useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Preview } from "./components/Preview";
import { NetworkHealth } from "./components/NetworkHealth";
import { EncodeSettings } from "./components/EncodeSettings";
import { Onboarding } from "./components/Onboarding";
import { OBSGuide } from "./components/OBSGuide";
import { NodeTree } from "./components/NodeTree";
import { EarningsPanel } from "./components/EarningsPanel";

export default function App() {
  const [onboardingDone, setOnboardingDone] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [streamId, setStreamId] = useState<string | null>(null);
  const [preset, setPreset] = useState("high");
  const [hwAccel, setHwAccel] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showOBSGuide, setShowOBSGuide] = useState(false);

  async function handleGoLive() {
    setError(null);
    if (!isTauri()) {
      setIsStreaming(true);
      setStreamId("dev-mock-stream-id");
      return;
    }
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
    if (!isTauri()) {
      setIsStreaming(false);
      setStreamId(null);
      return;
    }
    try {
      await invoke("stop_stream");
      setStreamId(null);
      setIsStreaming(false);
    } catch (err) {
      setError(String(err));
    }
  }

  if (!onboardingDone) {
    return <Onboarding onComplete={() => setOnboardingDone(true)} />;
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
      {showOBSGuide && <OBSGuide onClose={() => setShowOBSGuide(false)} />}

      {/* Left column: preview + node tree */}
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
        <NodeTree />
      </div>

      {/* Right column: controls + earnings */}
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
        <EarningsPanel channelState={null} />
        <button
          style={{
            padding: "8px 14px",
            background: "transparent",
            border: "1px solid #2d3748",
            borderRadius: 6,
            color: "#718096",
            fontSize: 12,
            cursor: "pointer",
            textAlign: "left",
          }}
          onClick={() => setShowOBSGuide(true)}
        >
          📡 Como conectar o OBS?
        </button>
      </div>
    </div>
  );
}
