import { useState } from "react";
import { useIdentity, Identity } from "./useIdentity";

interface IdentityModalProps {
  onIdentityReady: (identity: Identity) => void;
  onClose: () => void;
}

const WARNING_MESSAGE =
  "Sua identidade é armazenada apenas neste browser. " +
  "Se você limpar os dados do browser sem fazer backup, ela será perdida permanentemente.";

export function IdentityModal({ onIdentityReady, onClose }: IdentityModalProps) {
  const [displayName, setDisplayName] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { create, load, export: exportFn } = useIdentity();

  async function handleCreate() {
    if (!displayName.trim()) {
      setError("Display name is required.");
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const identity = await create(displayName.trim());
      onIdentityReady(identity);
    } catch (err) {
      setError(`Failed to create identity: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleLoad() {
    setLoading(true);
    setError(null);
    try {
      const identity = await load();
      if (!identity) {
        setError("No saved identity found. Create one instead.");
      } else {
        onIdentityReady(identity);
      }
    } catch (err) {
      setError(`Failed to load identity: ${err}`);
    } finally {
      setLoading(false);
    }
  }

  async function handleExport() {
    try {
      const blob = await exportFn();
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "prism-identity.json";
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(`Export failed: ${err}`);
    }
  }

  return (
    <div style={overlayStyle}>
      <div style={modalStyle}>
        <h2 style={{ marginTop: 0, color: "#e2e8f0" }}>Prism Identity</h2>

        <p style={{ color: "#fc8181", fontSize: 13, background: "#2d1b1b", padding: 10, borderRadius: 6 }}>
          ⚠ {WARNING_MESSAGE}
        </p>

        <label style={labelStyle}>Display Name</label>
        <input
          type="text"
          maxLength={32}
          value={displayName}
          onChange={(e) => setDisplayName(e.target.value)}
          placeholder="Enter your display name"
          style={inputStyle}
          disabled={loading}
        />

        <div style={{ display: "flex", gap: 8, marginTop: 16 }}>
          <button onClick={handleCreate} disabled={loading} style={primaryButtonStyle}>
            {loading ? "Creating…" : "Create New Identity"}
          </button>
          <button onClick={handleLoad} disabled={loading} style={secondaryButtonStyle}>
            Load Existing
          </button>
          <button onClick={handleExport} disabled={loading} style={secondaryButtonStyle}>
            Export Backup
          </button>
        </div>

        {error && <p style={{ color: "#fc8181", fontSize: 13, marginTop: 8 }}>{error}</p>}

        <button onClick={onClose} style={{ ...secondaryButtonStyle, marginTop: 16, width: "100%" }}>
          Cancel
        </button>
      </div>
    </div>
  );
}

const overlayStyle: React.CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "rgba(0,0,0,0.7)",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  zIndex: 1000,
};

const modalStyle: React.CSSProperties = {
  background: "#1a202c",
  border: "1px solid #4a5568",
  borderRadius: 10,
  padding: 28,
  width: 420,
  maxWidth: "95vw",
};

const labelStyle: React.CSSProperties = {
  display: "block",
  fontSize: 13,
  color: "#a0aec0",
  marginBottom: 6,
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "8px 12px",
  background: "#2d3748",
  color: "#e2e8f0",
  border: "1px solid #4a5568",
  borderRadius: 6,
  fontSize: 14,
  boxSizing: "border-box",
};

const primaryButtonStyle: React.CSSProperties = {
  flex: 1,
  padding: "9px 0",
  background: "#3182ce",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontWeight: 600,
  fontSize: 14,
};

const secondaryButtonStyle: React.CSSProperties = {
  flex: 1,
  padding: "9px 0",
  background: "#2d3748",
  color: "#e2e8f0",
  border: "1px solid #4a5568",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 14,
};
