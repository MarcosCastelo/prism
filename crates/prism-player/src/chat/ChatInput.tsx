import { useState } from "react";
import { Identity } from "../identity/useIdentity";

interface ChatInputProps {
  identity: Identity | null;
  onSend: (text: string) => void;
  onRequestIdentity: () => void;
  disabled?: boolean;
}

const MAX_CHARS = 500;

export function ChatInput({ identity, onSend, onRequestIdentity, disabled }: ChatInputProps) {
  const [text, setText] = useState("");

  function handleSend() {
    const trimmed = text.trim();
    if (!trimmed || !identity) return;
    onSend(trimmed);
    setText("");
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  if (!identity) {
    return (
      <div style={noIdentityContainerStyle}>
        <button onClick={onRequestIdentity} style={identityButtonStyle}>
          Create identity to chat
        </button>
      </div>
    );
  }

  return (
    <div style={containerStyle}>
      <span style={nameTagStyle}>{identity.displayName}</span>
      <input
        type="text"
        value={text}
        onChange={(e) => setText(e.target.value.slice(0, MAX_CHARS))}
        onKeyDown={handleKeyDown}
        placeholder="Type a message…"
        style={inputStyle}
        disabled={disabled}
        maxLength={MAX_CHARS}
      />
      <span style={charCountStyle}>{text.length}/{MAX_CHARS}</span>
      <button
        onClick={handleSend}
        disabled={disabled || !text.trim()}
        style={sendButtonStyle}
      >
        Send
      </button>
    </div>
  );
}

const containerStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "8px 10px",
  borderTop: "1px solid #2d3748",
  background: "#171923",
};

const noIdentityContainerStyle: React.CSSProperties = {
  padding: "10px 10px",
  borderTop: "1px solid #2d3748",
  background: "#171923",
  display: "flex",
  justifyContent: "center",
};

const nameTagStyle: React.CSSProperties = {
  fontSize: 12,
  color: "#63b3ed",
  fontWeight: 600,
  whiteSpace: "nowrap",
  flexShrink: 0,
};

const inputStyle: React.CSSProperties = {
  flex: 1,
  padding: "6px 10px",
  background: "#2d3748",
  color: "#e2e8f0",
  border: "1px solid #4a5568",
  borderRadius: 6,
  fontSize: 13,
  outline: "none",
};

const charCountStyle: React.CSSProperties = {
  fontSize: 11,
  color: "#4a5568",
  flexShrink: 0,
};

const sendButtonStyle: React.CSSProperties = {
  padding: "6px 14px",
  background: "#3182ce",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontWeight: 600,
  fontSize: 13,
  flexShrink: 0,
};

const identityButtonStyle: React.CSSProperties = {
  padding: "7px 18px",
  background: "#2d3748",
  color: "#63b3ed",
  border: "1px solid #4a5568",
  borderRadius: 6,
  cursor: "pointer",
  fontSize: 13,
};
