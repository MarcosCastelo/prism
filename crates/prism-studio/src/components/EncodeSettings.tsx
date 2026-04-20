interface EncodeSettingsProps {
  preset: string;
  hwAccel: boolean;
  onPresetChange: (preset: string) => void;
  onHwAccelChange: (enabled: boolean) => void;
  disabled: boolean;
}

const PRESETS = [
  { value: "low",    label: "Baixo",  desc: "360p · 400 kbps" },
  { value: "medium", label: "Médio",  desc: "480p · 1 Mbps" },
  { value: "high",   label: "Alto",   desc: "720p · 2.5 Mbps" },
  { value: "ultra",  label: "Ultra",  desc: "1080p · 5.5 Mbps" },
];

export function EncodeSettings({ preset, hwAccel, onPresetChange, onHwAccelChange, disabled }: EncodeSettingsProps) {
  return (
    <div style={{ padding: 16, background: "#1a202c", borderRadius: 8, color: "#e2e8f0" }}>
      <h3 style={{ margin: "0 0 12px", fontSize: 14, textTransform: "uppercase", color: "#a0aec0" }}>
        Encode Settings
      </h3>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8, marginBottom: 16 }}>
        {PRESETS.map((p) => (
          <button
            key={p.value}
            onClick={() => onPresetChange(p.value)}
            disabled={disabled}
            style={{
              padding: "10px 8px",
              background: preset === p.value ? "#3182ce" : "#2d3748",
              color: "#fff",
              border: "none",
              borderRadius: 6,
              cursor: disabled ? "not-allowed" : "pointer",
              textAlign: "left",
            }}
          >
            <div style={{ fontWeight: 600 }}>{p.label}</div>
            <div style={{ fontSize: 11, color: "#a0aec0" }}>{p.desc}</div>
          </button>
        ))}
      </div>

      <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
        <input
          type="checkbox"
          checked={hwAccel}
          onChange={(e) => onHwAccelChange(e.target.checked)}
          disabled={disabled}
        />
        <span style={{ fontSize: 13 }}>Aceleração de hardware (VAAPI / NVENC / VideoToolbox)</span>
      </label>
    </div>
  );
}
