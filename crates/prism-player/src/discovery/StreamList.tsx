import { useState } from "react";
import { useDhtDiscovery, StreamInfo } from "./useDhtDiscovery";

interface StreamListProps {
  onSelectStream: (streamId: string, manifestUrl: string) => void;
}

export function StreamList({ onSelectStream }: StreamListProps) {
  const [query, setQuery] = useState("");
  const { streams, isLoading, search } = useDhtDiscovery();

  function handleSearch(e: React.FormEvent) {
    e.preventDefault();
    search(query.trim());
  }

  return (
    <div style={containerStyle}>
      <form onSubmit={handleSearch} style={searchRowStyle}>
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search by stream title or pubkey…"
          style={searchInputStyle}
        />
        <button type="submit" style={searchButtonStyle}>
          Search
        </button>
      </form>

      {isLoading ? (
        <div style={statusStyle}>Discovering streams…</div>
      ) : streams.length === 0 ? (
        <div style={statusStyle}>No active streams found</div>
      ) : (
        <div style={gridStyle}>
          {streams.map((stream) => (
            <StreamCard
              key={stream.streamId}
              stream={stream}
              onSelect={onSelectStream}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function StreamCard({
  stream,
  onSelect,
}: {
  stream: StreamInfo;
  onSelect: (streamId: string, manifestUrl: string) => void;
}) {
  // Edge node provides the HLS manifest — the full URL is resolved at runtime
  const manifestUrl = `/streams/${stream.streamId}/master.m3u8`;

  const elapsed = Math.floor((Date.now() - stream.startedAt) / 1000);
  const uptime =
    elapsed < 60
      ? `${elapsed}s`
      : elapsed < 3600
        ? `${Math.floor(elapsed / 60)}m`
        : `${Math.floor(elapsed / 3600)}h`;

  return (
    <div
      style={cardStyle}
      onClick={() => onSelect(stream.streamId, manifestUrl)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => e.key === "Enter" && onSelect(stream.streamId, manifestUrl)}
    >
      <div style={thumbnailContainerStyle}>
        {stream.thumbnailUrl ? (
          <img
            src={stream.thumbnailUrl}
            alt={stream.title}
            style={thumbnailStyle}
          />
        ) : (
          <div style={thumbnailPlaceholderStyle}>▶</div>
        )}
        <span style={viewerBadgeStyle}>{stream.viewerCount} viewers</span>
        <span style={uptimeBadgeStyle}>{uptime}</span>
      </div>
      <div style={cardBodyStyle}>
        <div style={titleStyle}>{stream.title || "Untitled Stream"}</div>
        <div style={streamerStyle}>{stream.streamerDisplayName}</div>
      </div>
    </div>
  );
}

const containerStyle: React.CSSProperties = {
  padding: "16px 0",
};

const searchRowStyle: React.CSSProperties = {
  display: "flex",
  gap: 8,
  marginBottom: 16,
};

const searchInputStyle: React.CSSProperties = {
  flex: 1,
  padding: "8px 12px",
  background: "#2d3748",
  color: "#e2e8f0",
  border: "1px solid #4a5568",
  borderRadius: 6,
  fontSize: 14,
};

const searchButtonStyle: React.CSSProperties = {
  padding: "8px 18px",
  background: "#3182ce",
  color: "#fff",
  border: "none",
  borderRadius: 6,
  cursor: "pointer",
  fontWeight: 600,
};

const statusStyle: React.CSSProperties = {
  color: "#718096",
  fontSize: 14,
  textAlign: "center",
  padding: "32px 0",
};

const gridStyle: React.CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fill, minmax(220px, 1fr))",
  gap: 16,
};

const cardStyle: React.CSSProperties = {
  background: "#1a202c",
  borderRadius: 8,
  overflow: "hidden",
  border: "1px solid #2d3748",
  cursor: "pointer",
  transition: "border-color 0.15s",
};

const thumbnailContainerStyle: React.CSSProperties = {
  position: "relative",
  aspectRatio: "16/9",
  background: "#171923",
};

const thumbnailStyle: React.CSSProperties = {
  width: "100%",
  height: "100%",
  objectFit: "cover",
};

const thumbnailPlaceholderStyle: React.CSSProperties = {
  width: "100%",
  height: "100%",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  fontSize: 28,
  color: "#4a5568",
};

const viewerBadgeStyle: React.CSSProperties = {
  position: "absolute",
  bottom: 6,
  left: 6,
  background: "rgba(0,0,0,0.7)",
  color: "#e2e8f0",
  fontSize: 11,
  padding: "2px 6px",
  borderRadius: 4,
};

const uptimeBadgeStyle: React.CSSProperties = {
  position: "absolute",
  bottom: 6,
  right: 6,
  background: "#c53030",
  color: "#fff",
  fontSize: 11,
  padding: "2px 6px",
  borderRadius: 4,
  fontWeight: 700,
};

const cardBodyStyle: React.CSSProperties = {
  padding: "10px 12px",
};

const titleStyle: React.CSSProperties = {
  fontSize: 14,
  fontWeight: 600,
  color: "#e2e8f0",
  marginBottom: 4,
  overflow: "hidden",
  whiteSpace: "nowrap",
  textOverflow: "ellipsis",
};

const streamerStyle: React.CSSProperties = {
  fontSize: 12,
  color: "#718096",
};
