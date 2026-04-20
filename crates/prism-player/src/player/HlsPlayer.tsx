import { useEffect, useRef, useState } from "react";
import Hls, { Level } from "hls.js";

export interface HlsPlayerProps {
  manifestUrl: string;
  streamTitle: string;
}

type QualityLabel = "Auto" | string;

export function HlsPlayer({ manifestUrl, streamTitle }: HlsPlayerProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const hlsRef = useRef<Hls | null>(null);
  const [currentQuality, setCurrentQuality] = useState<QualityLabel>("Auto");
  const [levels, setLevels] = useState<Level[]>([]);
  const [manualLevel, setManualLevel] = useState<number>(-1); // -1 = ABR auto
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const video = videoRef.current;
    if (!video || !manifestUrl) return;

    setError(null);

    // Native HLS (Safari) — no JS needed.
    if (!Hls.isSupported() && video.canPlayType("application/vnd.apple.mpegurl")) {
      video.src = manifestUrl;
      video.play().catch(() => {});
      return;
    }

    if (!Hls.isSupported()) {
      setError("This browser does not support HLS playback.");
      return;
    }

    const hls = new Hls({ enableWorker: true, lowLatencyMode: true });
    hlsRef.current = hls;

    hls.loadSource(manifestUrl);
    hls.attachMedia(video);

    hls.on(Hls.Events.MANIFEST_PARSED, (_event, data) => {
      setLevels(data.levels);
      video.play().catch(() => {});
    });

    hls.on(Hls.Events.LEVEL_SWITCHED, (_event, data) => {
      const lvl = hls.levels[data.level];
      if (lvl) {
        setCurrentQuality(lvl.height ? `${lvl.height}p` : `Level ${data.level}`);
      }
    });

    hls.on(Hls.Events.ERROR, (_event, data) => {
      if (data.fatal) {
        switch (data.type) {
          case Hls.ErrorTypes.NETWORK_ERROR:
            hls.startLoad();
            break;
          case Hls.ErrorTypes.MEDIA_ERROR:
            hls.recoverMediaError();
            break;
          default:
            setError(`Playback error: ${data.details}`);
            hls.destroy();
        }
      }
    });

    return () => {
      hls.destroy();
      hlsRef.current = null;
    };
  }, [manifestUrl]);

  function handleQualityChange(levelIndex: number) {
    const hls = hlsRef.current;
    if (!hls) return;
    setManualLevel(levelIndex);
    hls.currentLevel = levelIndex; // -1 = ABR, ≥0 = fixed quality
    setCurrentQuality(levelIndex === -1 ? "Auto" : `${hls.levels[levelIndex]?.height ?? levelIndex}p`);
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ position: "relative", background: "#000", borderRadius: 8, overflow: "hidden" }}>
        <video
          ref={videoRef}
          controls
          playsInline
          style={{ width: "100%", display: "block" }}
          aria-label={streamTitle}
        />
        <div
          style={{
            position: "absolute",
            bottom: 48,
            right: 8,
            background: "rgba(0,0,0,0.7)",
            color: "#fff",
            fontSize: 12,
            padding: "3px 8px",
            borderRadius: 4,
            pointerEvents: "none",
          }}
        >
          {currentQuality}
        </div>
      </div>

      {error && (
        <div style={{ color: "#fc8181", fontSize: 13, padding: "6px 10px", background: "#2d2020", borderRadius: 6 }}>
          {error}
        </div>
      )}

      {levels.length > 0 && (
        <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
          <QualityButton
            label="Auto"
            active={manualLevel === -1}
            onClick={() => handleQualityChange(-1)}
          />
          {levels.map((lvl, i) => (
            <QualityButton
              key={i}
              label={lvl.height ? `${lvl.height}p` : `L${i}`}
              active={manualLevel === i}
              onClick={() => handleQualityChange(i)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function QualityButton({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      style={{
        padding: "4px 10px",
        fontSize: 12,
        background: active ? "#3182ce" : "#2d3748",
        color: "#fff",
        border: "none",
        borderRadius: 4,
        cursor: "pointer",
      }}
    >
      {label}
    </button>
  );
}
