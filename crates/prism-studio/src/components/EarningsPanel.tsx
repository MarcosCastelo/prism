import { useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

interface ChannelState {
  channel_id: string;
  payer_pubkey: string;
  payee_pubkey: string;
  sequence: number;
  payer_balance: number;
  payee_balance: number;
}

interface EarningsPanelProps {
  channelState: ChannelState | null;
}

const COLORS = {
  bg: "#171923",
  card: "#1a2035",
  border: "#2d3748",
  accent: "#4f9cf9",
  success: "#68d391",
  warn: "#f6ad55",
  text: "#e2e8f0",
  muted: "#718096",
};

const UNIT_LABEL = "PRISMu"; // micro-PRISM placeholder

export function EarningsPanel({ channelState }: EarningsPanelProps) {
  const [settling, setSettling] = useState(false);
  const [settled, setSettled] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleSettle() {
    setSettling(true);
    setError(null);
    try {
      const txHash = isTauri()
        ? await invoke<string>("settle_channel")
        : "mock_tx_" + Math.random().toString(16).slice(2, 10);
      setSettled(txHash);
    } catch (e) {
      setError(String(e));
    } finally {
      setSettling(false);
    }
  }

  if (!channelState) {
    return (
      <div style={styles.container}>
        <span style={styles.title}>Ganhos</span>
        <div style={styles.empty}>
          <span style={{ fontSize: 28 }}>💰</span>
          <span style={{ color: COLORS.muted, fontSize: 13 }}>
            Nenhum state channel aberto.
            <br />
            Ganhos aparecem aqui durante a live.
          </span>
        </div>
      </div>
    );
  }

  const earnedDisplay = channelState.payee_balance.toLocaleString();
  const remainingDisplay = channelState.payer_balance.toLocaleString();
  const totalDisplay = (channelState.payer_balance + channelState.payee_balance).toLocaleString();
  const progressPct =
    channelState.payer_balance + channelState.payee_balance > 0
      ? (channelState.payee_balance / (channelState.payer_balance + channelState.payee_balance)) * 100
      : 0;

  return (
    <div style={styles.container}>
      <div style={styles.header}>
        <span style={styles.title}>Ganhos</span>
        <span style={styles.seq}>seq #{channelState.sequence}</span>
      </div>

      <div style={styles.earned}>
        <span style={styles.earnedLabel}>Ganho nesta sessão</span>
        <span style={styles.earnedValue}>
          {earnedDisplay}{" "}
          <span style={styles.unit}>{UNIT_LABEL}</span>
        </span>
      </div>

      <div style={styles.progressBar}>
        <div
          style={{
            ...styles.progressFill,
            width: `${progressPct}%`,
          }}
        />
      </div>

      <div style={styles.balances}>
        <BalanceItem label="Recebido" value={earnedDisplay} color={COLORS.success} />
        <BalanceItem label="Restante no canal" value={remainingDisplay} color={COLORS.muted} />
        <BalanceItem label="Total do canal" value={totalDisplay} color={COLORS.text} />
      </div>

      <div style={styles.channelInfo}>
        <span style={styles.channelLabel}>Canal</span>
        <span style={styles.channelId}>
          {channelState.channel_id.slice(0, 8)}...{channelState.channel_id.slice(-8)}
        </span>
      </div>

      {error && (
        <div style={styles.errorBox}>{error}</div>
      )}

      {settled ? (
        <div style={styles.successBox}>
          <span style={{ color: COLORS.success, fontWeight: 600 }}>✓ Canal liquidado</span>
          <code style={{ color: COLORS.muted, fontSize: 11, wordBreak: "break-all" }}>
            tx: {settled}
          </code>
        </div>
      ) : (
        <button
          style={{
            ...styles.settleBtn,
            opacity: settling ? 0.7 : 1,
            cursor: settling ? "not-allowed" : "pointer",
          }}
          onClick={handleSettle}
          disabled={settling}
        >
          {settling ? "Liquidando..." : "Fechar canal e liquidar"}
        </button>
      )}
    </div>
  );
}

function BalanceItem({
  label,
  value,
  color,
}: {
  label: string;
  value: string;
  color: string;
}) {
  return (
    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
      <span style={{ color: COLORS.muted, fontSize: 12 }}>{label}</span>
      <span style={{ color, fontSize: 13, fontWeight: 600, fontFamily: "monospace" }}>
        {value}
      </span>
    </div>
  );
}

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
  seq: {
    color: COLORS.muted,
    fontSize: 11,
    fontFamily: "monospace",
  },
  empty: {
    display: "flex",
    flexDirection: "column",
    alignItems: "center",
    gap: 8,
    padding: "20px 0",
    textAlign: "center",
  },
  earned: {
    display: "flex",
    flexDirection: "column",
    gap: 2,
  },
  earnedLabel: {
    color: COLORS.muted,
    fontSize: 11,
    textTransform: "uppercase",
    letterSpacing: "0.05em",
  },
  earnedValue: {
    color: COLORS.success,
    fontSize: 28,
    fontWeight: 700,
    fontFamily: "monospace",
  },
  unit: {
    fontSize: 14,
    fontWeight: 400,
    color: COLORS.muted,
  },
  progressBar: {
    height: 4,
    background: COLORS.border,
    borderRadius: 2,
    overflow: "hidden",
  },
  progressFill: {
    height: "100%",
    background: COLORS.success,
    borderRadius: 2,
    transition: "width 0.5s ease",
  },
  balances: {
    display: "flex",
    flexDirection: "column",
    gap: 6,
    padding: "8px 0",
    borderTop: `1px solid ${COLORS.border}`,
    borderBottom: `1px solid ${COLORS.border}`,
  },
  channelInfo: {
    display: "flex",
    gap: 8,
    alignItems: "center",
  },
  channelLabel: {
    color: COLORS.muted,
    fontSize: 11,
    textTransform: "uppercase",
    letterSpacing: "0.05em",
    flexShrink: 0,
  },
  channelId: {
    color: COLORS.muted,
    fontFamily: "monospace",
    fontSize: 11,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
  settleBtn: {
    width: "100%",
    padding: "10px",
    background: "transparent",
    border: `1px solid ${COLORS.warn}`,
    borderRadius: 8,
    color: COLORS.warn,
    fontSize: 13,
    fontWeight: 600,
  },
  errorBox: {
    padding: "8px 12px",
    background: "#2d2020",
    border: "1px solid #742a2a",
    borderRadius: 6,
    color: "#fc8181",
    fontSize: 12,
  },
  successBox: {
    display: "flex",
    flexDirection: "column",
    gap: 4,
    padding: "10px 12px",
    background: "#1a2b1a",
    border: `1px solid ${COLORS.success}`,
    borderRadius: 8,
  },
};
