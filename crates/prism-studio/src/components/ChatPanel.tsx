import { useState, useEffect, useRef } from "react";

export interface StudioChatMessage {
  id: string;
  displayName: string;
  text: string;
  timestampMs: number;
  senderPubkeyHex: string;
}

interface ChatPanelProps {
  streamId: string | null;
  onBlockUser?: (pubkeyHex: string, displayName: string) => void;
}

export function ChatPanel({ streamId, onBlockUser }: ChatPanelProps) {
  const [messages, setMessages] = useState<StudioChatMessage[]>([]);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    msg: StudioChatMessage;
  } | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  // In production: subscribe to chat events from prism-node via Tauri events.
  // window.__TAURI__.event.listen('chat-message', (event) => {
  //   setMessages(prev => [...prev.slice(-499), event.payload]);
  // });

  function handleRightClick(e: React.MouseEvent, msg: StudioChatMessage) {
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, msg });
  }

  function handleBlock() {
    if (!contextMenu) return;
    onBlockUser?.(contextMenu.msg.senderPubkeyHex, contextMenu.msg.displayName);
    setContextMenu(null);
  }

  return (
    <div style={containerStyle} onClick={() => setContextMenu(null)}>
      <div style={headerStyle}>
        <span>Chat</span>
        {streamId && (
          <span style={streamIdStyle}>{streamId.slice(0, 8)}…</span>
        )}
      </div>

      <div style={messageListStyle}>
        {messages.length === 0 ? (
          <div style={emptyStyle}>
            {streamId ? "No messages yet" : "Start streaming to enable chat"}
          </div>
        ) : (
          messages.map((msg) => (
            <div
              key={msg.id}
              style={messageRowStyle}
              onContextMenu={(e) => handleRightClick(e, msg)}
            >
              <span style={timeStyle}>
                {new Date(msg.timestampMs).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </span>
              <span style={nameStyle}>{msg.displayName}</span>
              <span style={textStyle}>{msg.text}</span>
            </div>
          ))
        )}
        <div ref={bottomRef} />
      </div>

      {contextMenu && (
        <div
          style={{
            ...contextMenuStyle,
            left: contextMenu.x,
            top: contextMenu.y,
          }}
        >
          <div style={contextMenuItemStyle} onClick={handleBlock}>
            Block {contextMenu.msg.displayName}
          </div>
        </div>
      )}
    </div>
  );
}

const containerStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  height: "100%",
  background: "#1a202c",
  borderRadius: 8,
  border: "1px solid #2d3748",
  overflow: "hidden",
  position: "relative",
};

const headerStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  padding: "10px 14px",
  fontSize: 13,
  fontWeight: 600,
  color: "#a0aec0",
  borderBottom: "1px solid #2d3748",
  background: "#171923",
};

const streamIdStyle: React.CSSProperties = {
  fontSize: 11,
  color: "#4a5568",
  fontFamily: "monospace",
};

const messageListStyle: React.CSSProperties = {
  flex: 1,
  overflowY: "auto",
  padding: "8px 4px",
  display: "flex",
  flexDirection: "column",
  gap: 2,
};

const emptyStyle: React.CSSProperties = {
  color: "#4a5568",
  fontSize: 13,
  textAlign: "center",
  marginTop: 24,
};

const messageRowStyle: React.CSSProperties = {
  padding: "3px 10px",
  fontSize: 13,
  lineHeight: 1.5,
  wordBreak: "break-word",
  cursor: "default",
  userSelect: "text",
};

const timeStyle: React.CSSProperties = {
  color: "#4a5568",
  fontSize: 11,
  marginRight: 6,
};

const nameStyle: React.CSSProperties = {
  color: "#63b3ed",
  fontWeight: 600,
  marginRight: 6,
};

const textStyle: React.CSSProperties = {
  color: "#e2e8f0",
};

const contextMenuStyle: React.CSSProperties = {
  position: "fixed",
  background: "#2d3748",
  border: "1px solid #4a5568",
  borderRadius: 6,
  zIndex: 1000,
  minWidth: 160,
  boxShadow: "0 4px 12px rgba(0,0,0,0.4)",
};

const contextMenuItemStyle: React.CSSProperties = {
  padding: "9px 14px",
  fontSize: 13,
  color: "#fc8181",
  cursor: "pointer",
};
