import { useState } from "react";

export interface BlockedUser {
  pubkeyHex: string;
  displayName: string;
  blockedAt: number;
  reason: string;
}

interface ModerationMenuProps {
  blockedUsers: BlockedUser[];
  onBlock: (pubkeyHex: string, displayName: string, reason: string) => void;
  onUnblock: (pubkeyHex: string) => void;
}

export function ModerationMenu({ blockedUsers, onBlock, onUnblock }: ModerationMenuProps) {
  const [pubkeyInput, setPubkeyInput] = useState("");
  const [reasonInput, setReasonInput] = useState("");
  const [nameInput, setNameInput] = useState("");
  const [error, setError] = useState<string | null>(null);

  function handleBlock(e: React.FormEvent) {
    e.preventDefault();
    const key = pubkeyInput.trim();
    if (!key || key.length < 64) {
      setError("Enter a valid Ed25519 public key (64+ hex chars).");
      return;
    }
    setError(null);
    onBlock(key, nameInput.trim() || key.slice(0, 8), reasonInput.trim());
    setPubkeyInput("");
    setNameInput("");
    setReasonInput("");
  }

  return (
    <div style={containerStyle}>
      <h3 style={headingStyle}>Moderation</h3>

      <form onSubmit={handleBlock} style={formStyle}>
        <label style={labelStyle}>Block by Public Key</label>
        <input
          type="text"
          value={pubkeyInput}
          onChange={(e) => setPubkeyInput(e.target.value)}
          placeholder="Ed25519 pubkey hex"
          style={inputStyle}
        />
        <input
          type="text"
          value={nameInput}
          onChange={(e) => setNameInput(e.target.value)}
          placeholder="Display name (optional)"
          style={{ ...inputStyle, marginTop: 4 }}
        />
        <input
          type="text"
          value={reasonInput}
          onChange={(e) => setReasonInput(e.target.value.slice(0, 100))}
          placeholder="Reason (optional, max 100 chars)"
          style={{ ...inputStyle, marginTop: 4 }}
        />
        {error && <p style={errorStyle}>{error}</p>}
        <button type="submit" style={blockButtonStyle}>
          Block User
        </button>
      </form>

      <div style={listHeaderStyle}>
        Blocked Users ({blockedUsers.length})
      </div>

      {blockedUsers.length === 0 ? (
        <div style={emptyStyle}>No blocked users</div>
      ) : (
        <div style={listStyle}>
          {blockedUsers.map((user) => (
            <BlockedUserRow key={user.pubkeyHex} user={user} onUnblock={onUnblock} />
          ))}
        </div>
      )}
    </div>
  );
}

function BlockedUserRow({
  user,
  onUnblock,
}: {
  user: BlockedUser;
  onUnblock: (pubkeyHex: string) => void;
}) {
  return (
    <div style={rowStyle}>
      <div style={rowInfoStyle}>
        <span style={rowNameStyle}>{user.displayName}</span>
        <span style={rowKeyStyle}>{user.pubkeyHex.slice(0, 16)}…</span>
        {user.reason && (
          <span style={rowReasonStyle}>"{user.reason}"</span>
        )}
      </div>
      <button
        onClick={() => onUnblock(user.pubkeyHex)}
        style={unblockButtonStyle}
      >
        Unblock
      </button>
    </div>
  );
}

const containerStyle: React.CSSProperties = {
  background: "#1a202c",
  borderRadius: 8,
  border: "1px solid #2d3748",
  padding: 16,
  fontSize: 13,
  color: "#e2e8f0",
};

const headingStyle: React.CSSProperties = {
  margin: "0 0 14px 0",
  fontSize: 15,
  fontWeight: 700,
  color: "#e2e8f0",
};

const formStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 0,
  marginBottom: 18,
};

const labelStyle: React.CSSProperties = {
  fontSize: 12,
  color: "#a0aec0",
  marginBottom: 6,
};

const inputStyle: React.CSSProperties = {
  padding: "7px 10px",
  background: "#2d3748",
  color: "#e2e8f0",
  border: "1px solid #4a5568",
  borderRadius: 6,
  fontSize: 13,
};

const errorStyle: React.CSSProperties = {
  color: "#fc8181",
  fontSize: 12,
  margin: "4px 0 0",
};

const blockButtonStyle: React.CSSProperties = {
  marginTop: 8,
  padding: "7px 16px",
  background: "#c53030",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontWeight: 600,
  alignSelf: "flex-start",
};

const listHeaderStyle: React.CSSProperties = {
  fontSize: 12,
  color: "#a0aec0",
  fontWeight: 600,
  marginBottom: 8,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const emptyStyle: React.CSSProperties = {
  color: "#4a5568",
  fontSize: 13,
};

const listStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
};

const rowStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: "8px 10px",
  background: "#171923",
  borderRadius: 6,
  border: "1px solid #2d3748",
};

const rowInfoStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 2,
  flex: 1,
  minWidth: 0,
};

const rowNameStyle: React.CSSProperties = {
  fontWeight: 600,
  color: "#e2e8f0",
};

const rowKeyStyle: React.CSSProperties = {
  fontFamily: "monospace",
  fontSize: 11,
  color: "#718096",
};

const rowReasonStyle: React.CSSProperties = {
  fontSize: 11,
  color: "#a0aec0",
  fontStyle: "italic",
};

const unblockButtonStyle: React.CSSProperties = {
  padding: "5px 12px",
  background: "transparent",
  color: "#68d391",
  border: "1px solid #276749",
  borderRadius: 5,
  cursor: "pointer",
  fontSize: 12,
  flexShrink: 0,
  marginLeft: 8,
};
