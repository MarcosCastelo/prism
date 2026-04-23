import { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";

interface NodeInfo {
  node_id: string;
  capacity_class: "A" | "B" | "C" | "edge";
  health_score: number;   // 0.0 – 1.0
  viewers: number;
  region: string;
  parent_id: string | null;
}

interface NodeTreeUpdate {
  nodes: NodeInfo[];
}

interface NodeTreeProps {
  onNodeClick?: (nodeId: string) => void;
}

const COLORS = {
  bg: "#171923",
  card: "#1a2035",
  border: "#2d3748",
  text: "#e2e8f0",
  muted: "#718096",
  healthy: "#68d391",
  warn: "#f6ad55",
  critical: "#fc8181",
};

function healthColor(score: number): string {
  if (score >= 0.7) return COLORS.healthy;
  if (score >= 0.4) return COLORS.warn;
  return COLORS.critical;
}

const CLASS_LABEL: Record<string, string> = {
  A: "Classe A",
  B: "Classe B",
  C: "Classe C",
  edge: "Edge",
};

const MOCK_NODES: NodeInfo[] = [
  { node_id: "a1b2c3d4", capacity_class: "A", health_score: 0.95, viewers: 0, region: "us-east", parent_id: null },
  { node_id: "e5f6a7b8", capacity_class: "B", health_score: 0.82, viewers: 12, region: "us-east", parent_id: "a1b2c3d4" },
  { node_id: "c9d0e1f2", capacity_class: "B", health_score: 0.61, viewers: 8, region: "eu-west", parent_id: "a1b2c3d4" },
  { node_id: "33445566", capacity_class: "C", health_score: 0.90, viewers: 4, region: "us-east", parent_id: "e5f6a7b8" },
  { node_id: "77889900", capacity_class: "edge", health_score: 0.35, viewers: 6, region: "ap-east", parent_id: "c9d0e1f2" },
];

export function NodeTree({ onNodeClick }: NodeTreeProps) {
  const [nodes, setNodes] = useState<NodeInfo[]>(MOCK_NODES);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<Date>(new Date());

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<NodeTreeUpdate>("node-tree-update", (event) => {
      setNodes(event.payload.nodes);
      setLastUpdated(new Date());
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const roots = nodes.filter((n) => n.parent_id === null);

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <span style={styles.title}>Rede de Relay</span>
        <span style={styles.updated}>
          Atualizado {lastUpdated.toLocaleTimeString()}
        </span>
      </div>

      <div style={styles.legend}>
        {Object.entries(CLASS_LABEL).map(([cls, label]) => (
          <span key={cls} style={styles.legendItem}>
            <span
              style={{
                ...styles.dot,
                background: cls === "A" ? "#9f7aea" : cls === "B" ? "#4f9cf9" : cls === "C" ? "#68d391" : "#f6ad55",
              }}
            />
            {label}
          </span>
        ))}
      </div>

      <div style={styles.treeArea}>
        {roots.map((root) => (
          <NodeBranch
            key={root.node_id}
            node={root}
            allNodes={nodes}
            depth={0}
            hoveredId={hoveredId}
            onHover={setHoveredId}
            onClick={onNodeClick}
          />
        ))}
      </div>

      <div style={styles.summary}>
        <SummaryPill label="Nós" value={nodes.length} />
        <SummaryPill
          label="Viewers"
          value={nodes.reduce((s, n) => s + n.viewers, 0)}
        />
        <SummaryPill
          label="Saúde média"
          value={`${Math.round((nodes.reduce((s, n) => s + n.health_score, 0) / nodes.length) * 100)}%`}
        />
      </div>
    </div>
  );
}

// ── Sub-components ─────────────────────────────────────────────────────────

function NodeBranch({
  node,
  allNodes,
  depth,
  hoveredId,
  onHover,
  onClick,
}: {
  node: NodeInfo;
  allNodes: NodeInfo[];
  depth: number;
  hoveredId: string | null;
  onHover: (id: string | null) => void;
  onClick?: (id: string) => void;
}) {
  const children = allNodes.filter((n) => n.parent_id === node.node_id);
  const color = healthColor(node.health_score);
  const isHovered = hoveredId === node.node_id;

  return (
    <div style={{ marginLeft: depth * 20 }}>
      <div
        style={{
          ...styles.nodeRow,
          background: isHovered ? "#1e2940" : "transparent",
          cursor: onClick ? "pointer" : "default",
        }}
        onMouseEnter={() => onHover(node.node_id)}
        onMouseLeave={() => onHover(null)}
        onClick={() => onClick?.(node.node_id)}
      >
        {depth > 0 && <span style={styles.connector}>└</span>}
        <span style={{ ...styles.healthDot, background: color }} />
        <span style={styles.nodeId}>{node.node_id}</span>
        <span style={styles.nodeClass}>{node.capacity_class.toUpperCase()}</span>
        <span style={styles.nodeRegion}>{node.region}</span>

        {isHovered && (
          <div style={styles.tooltip}>
            <div>Score: {Math.round(node.health_score * 100)}%</div>
            <div>Viewers: {node.viewers}</div>
            <div>Região: {node.region}</div>
          </div>
        )}
      </div>

      {children.map((child) => (
        <NodeBranch
          key={child.node_id}
          node={child}
          allNodes={allNodes}
          depth={depth + 1}
          hoveredId={hoveredId}
          onHover={onHover}
          onClick={onClick}
        />
      ))}
    </div>
  );
}

function SummaryPill({ label, value }: { label: string; value: string | number }) {
  return (
    <div style={styles.pill}>
      <span style={styles.pillLabel}>{label}</span>
      <span style={styles.pillValue}>{value}</span>
    </div>
  );
}

// ── Styles ───────────────────────────────────────────────────────────────────

const styles: Record<string, React.CSSProperties> = {
  container: {
    background: COLORS.card,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 10,
    padding: 16,
    display: "flex",
    flexDirection: "column",
    gap: 12,
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "center",
  },
  title: {
    color: COLORS.text,
    fontSize: 14,
    fontWeight: 600,
  },
  updated: {
    color: COLORS.muted,
    fontSize: 11,
  },
  legend: {
    display: "flex",
    gap: 12,
    flexWrap: "wrap",
  },
  legendItem: {
    display: "flex",
    alignItems: "center",
    gap: 5,
    color: COLORS.muted,
    fontSize: 11,
  },
  dot: {
    width: 8,
    height: 8,
    borderRadius: "50%",
    display: "inline-block",
  },
  treeArea: {
    fontFamily: "monospace",
    fontSize: 13,
    display: "flex",
    flexDirection: "column",
    gap: 2,
  },
  nodeRow: {
    display: "flex",
    alignItems: "center",
    gap: 8,
    padding: "4px 6px",
    borderRadius: 4,
    position: "relative",
  },
  connector: {
    color: COLORS.muted,
    lineHeight: 1,
  },
  healthDot: {
    width: 8,
    height: 8,
    borderRadius: "50%",
    flexShrink: 0,
  },
  nodeId: {
    color: COLORS.text,
    fontFamily: "monospace",
    fontSize: 13,
    flex: 1,
  },
  nodeClass: {
    color: COLORS.muted,
    fontSize: 11,
    background: "#2d3748",
    padding: "1px 6px",
    borderRadius: 3,
  },
  nodeRegion: {
    color: COLORS.muted,
    fontSize: 11,
  },
  tooltip: {
    position: "absolute",
    right: 0,
    top: "100%",
    background: "#0d1117",
    border: `1px solid ${COLORS.border}`,
    borderRadius: 6,
    padding: "8px 12px",
    zIndex: 50,
    fontSize: 12,
    color: COLORS.text,
    lineHeight: 1.7,
    whiteSpace: "nowrap",
    boxShadow: "0 4px 12px rgba(0,0,0,0.5)",
  },
  summary: {
    display: "flex",
    gap: 8,
    borderTop: `1px solid ${COLORS.border}`,
    paddingTop: 10,
  },
  pill: {
    background: COLORS.bg,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 6,
    padding: "6px 12px",
    display: "flex",
    flexDirection: "column",
    gap: 2,
    flex: 1,
    textAlign: "center",
  },
  pillLabel: {
    color: COLORS.muted,
    fontSize: 10,
    textTransform: "uppercase",
    letterSpacing: "0.05em",
  },
  pillValue: {
    color: COLORS.text,
    fontSize: 16,
    fontWeight: 700,
  },
};
