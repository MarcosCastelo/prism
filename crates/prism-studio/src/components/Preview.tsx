import { useEffect, useRef, useState } from "react";

interface PreviewProps {
  isStreaming: boolean;
}

interface PreviewStats {
  width: number;
  height: number;
  fps: number;
  cpuPercent: number;
}

export function Preview({ isStreaming }: PreviewProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [stats, setStats] = useState<PreviewStats>({ width: 1280, height: 720, fps: 15, cpuPercent: 0 });

  useEffect(() => {
    let stream: MediaStream | null = null;

    async function startPreview() {
      try {
        stream = await navigator.mediaDevices.getUserMedia({ video: true, audio: false });
        if (videoRef.current) {
          videoRef.current.srcObject = stream;
          const track = stream.getVideoTracks()[0];
          const settings = track.getSettings();
          setStats(prev => ({
            ...prev,
            width: settings.width ?? 1280,
            height: settings.height ?? 720,
          }));
        }
      } catch (err) {
        console.error("Failed to access camera:", err);
      }
    }

    startPreview();
    return () => {
      stream?.getTracks().forEach(t => t.stop());
    };
  }, []);

  // Poll CPU usage (simulated — real impl uses Tauri system-info plugin).
  useEffect(() => {
    const id = setInterval(() => {
      setStats(prev => ({ ...prev, cpuPercent: Math.floor(Math.random() * 30) + 20 }));
    }, 2000);
    return () => clearInterval(id);
  }, []);

  const isHighCpu = stats.cpuPercent > 85;

  return (
    <div style={{ position: "relative", background: "#000", borderRadius: 8, overflow: "hidden" }}>
      <video
        ref={videoRef}
        autoPlay
        muted
        playsInline
        style={{ width: "100%", display: "block" }}
      />
      <div
        style={{
          position: "absolute",
          bottom: 8,
          left: 8,
          background: "rgba(0,0,0,0.6)",
          color: "#fff",
          fontSize: 12,
          padding: "4px 8px",
          borderRadius: 4,
        }}
      >
        {stats.width}×{stats.height} · {stats.fps} fps
        {isStreaming && " · LIVE"}
      </div>
      {isHighCpu && (
        <div
          style={{
            position: "absolute",
            top: 8,
            right: 8,
            background: "#e53e3e",
            color: "#fff",
            fontSize: 12,
            padding: "4px 8px",
            borderRadius: 4,
          }}
        >
          ⚠ CPU {stats.cpuPercent}%
        </div>
      )}
    </div>
  );
}
