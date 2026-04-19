import { useState } from "react";
import { HlsPlayer } from "./player/HlsPlayer";

export default function App() {
  const [manifestUrl, setManifestUrl] = useState("");
  const [activeUrl, setActiveUrl] = useState("");
  const [streamTitle, setStreamTitle] = useState("Prism Stream");

  function handleWatch() {
    if (manifestUrl.trim()) {
      setActiveUrl(manifestUrl.trim());
    }
  }

  return (
    <div
      style={{
        maxWidth: 900,
        margin: "0 auto",
        padding: 24,
        fontFamily: "system-ui, sans-serif",
        background: "#171923",
        minHeight: "100vh",
        color: "#e2e8f0",
        boxSizing: "border-box",
      }}
    >
      <h1 style={{ fontSize: 20, marginBottom: 20, color: "#fff" }}>
        Prism Player
      </h1>

      <div style={{ display: "flex", gap: 8, marginBottom: 20 }}>
        <input
          type="text"
          value={manifestUrl}
          onChange={(e) => setManifestUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleWatch()}
          placeholder="Paste HLS manifest URL (.m3u8)"
          style={{
            flex: 1,
            padding: "8px 12px",
            background: "#2d3748",
            color: "#e2e8f0",
            border: "1px solid #4a5568",
            borderRadius: 6,
            fontSize: 14,
          }}
        />
        <button
          onClick={handleWatch}
          style={{
            padding: "8px 20px",
            background: "#3182ce",
            color: "#fff",
            border: "none",
            borderRadius: 6,
            cursor: "pointer",
            fontWeight: 600,
          }}
        >
          Watch
        </button>
      </div>

      {activeUrl ? (
        <HlsPlayer manifestUrl={activeUrl} streamTitle={streamTitle} />
      ) : (
        <div
          style={{
            height: 300,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "#1a202c",
            borderRadius: 8,
            color: "#718096",
            fontSize: 14,
          }}
        >
          Enter a stream URL above to begin playback
        </div>
      )}
    </div>
  );
}
