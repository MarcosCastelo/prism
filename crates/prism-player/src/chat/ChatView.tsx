import { useEffect, useRef } from "react";

export interface ChatMessageItem {
  id: string;
  displayName: string;
  text: string;
  timestampMs: number;
  senderPubkeyHex: string;
}

interface ChatViewProps {
  messages: ChatMessageItem[];
}

export function ChatView({ messages }: ChatViewProps) {
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  return (
    <div style={containerStyle}>
      <div style={headerStyle}>Chat</div>
      <div style={messageListStyle}>
        {messages.length === 0 ? (
          <div style={emptyStyle}>No messages yet</div>
        ) : (
          messages.map((msg) => <MessageRow key={msg.id} msg={msg} />)
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}

function MessageRow({ msg }: { msg: ChatMessageItem }) {
  const time = new Date(msg.timestampMs).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });

  return (
    <div style={messageRowStyle}>
      <span style={timeStyle}>{time}</span>
      <span style={nameStyle} title={msg.senderPubkeyHex}>
        {msg.displayName}
      </span>
      <span style={textStyle}>{msg.text}</span>
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
};

const headerStyle: React.CSSProperties = {
  padding: "10px 14px",
  fontSize: 13,
  fontWeight: 600,
  color: "#a0aec0",
  borderBottom: "1px solid #2d3748",
  background: "#171923",
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
};

const timeStyle: React.CSSProperties = {
  color: "#4a5568",
  fontSize: 11,
  marginRight: 6,
  flexShrink: 0,
};

const nameStyle: React.CSSProperties = {
  color: "#63b3ed",
  fontWeight: 600,
  marginRight: 6,
  cursor: "help",
};

const textStyle: React.CSSProperties = {
  color: "#e2e8f0",
};
