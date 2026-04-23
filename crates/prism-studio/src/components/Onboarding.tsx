import { useState, useEffect } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

interface OnboardingStatus {
  current_step: number;
  identity_pubkey: string | null;
  benchmark_result: string | null;
  source_type: string | null;
  completed: boolean;
}

interface OnboardingProps {
  onComplete: () => void;
}

const STEP_LABELS = [
  "Identidade",
  "Backup",
  "Benchmark",
  "Fonte de Vídeo",
  "Pronto!",
];

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

export function Onboarding({ onComplete }: OnboardingProps) {
  const [step, setStep] = useState(1);
  const [pubkey, setPubkey] = useState<string | null>(null);
  const [benchmarkResult, setBenchmarkResult] = useState<string | null>(null);
  const [sourceType, setSourceType] = useState<"camera" | "rtmp" | null>(null);
  const [password, setPassword] = useState("");
  const [backupDone, setBackupDone] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showTutorial, setShowTutorial] = useState(false);

  useEffect(() => {
    loadStatus();
  }, []);

  async function loadStatus() {
    if (!isTauri()) {
      return;
    }
    try {
      const status = await invoke<OnboardingStatus>("onboarding_status");
      if (status.completed) {
        onComplete();
        return;
      }
      setStep(status.current_step);
      setPubkey(status.identity_pubkey);
      setBenchmarkResult(status.benchmark_result);
      if (status.source_type) {
        setSourceType(status.source_type as "camera" | "rtmp");
      }
    } catch {
      // first run — start at step 1
    }
  }

  async function handleGenerateIdentity() {
    setLoading(true);
    setError(null);
    try {
      const display = isTauri()
        ? await invoke<string>("onboarding_generate_identity")
        : "pr1abcd...ef12";
      setPubkey(display);
      setStep(2);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleExportBackup() {
    if (!password.trim()) {
      setError("Digite uma senha para proteger o backup.");
      return;
    }
    setLoading(true);
    setError(null);
    try {
      if (isTauri()) {
        await invoke<string>("onboarding_export_backup", { password });
      }
      setBackupDone(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleRunBenchmark() {
    setLoading(true);
    setError(null);
    try {
      const result = isTauri()
        ? await invoke<string>("onboarding_run_benchmark")
        : "1080p60";
      setBenchmarkResult(result);
      setStep(4);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleSetSource(type: "camera" | "rtmp") {
    setLoading(true);
    setError(null);
    try {
      if (isTauri()) {
        await invoke("onboarding_set_source", { sourceType: type });
      }
      setSourceType(type);
      setStep(5);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleComplete() {
    setLoading(true);
    try {
      if (isTauri()) {
        await invoke("onboarding_complete");
      }
      setShowTutorial(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  if (showTutorial) {
    return (
      <TutorialOverlay
        onDismiss={onComplete}
      />
    );
  }

  return (
    <div style={styles.overlay}>
      <div style={styles.card}>
        <StepIndicator current={step} labels={STEP_LABELS} />
        <div style={styles.content}>
          {step === 1 && (
            <IdentityStep
              onGenerate={handleGenerateIdentity}
              loading={loading}
              error={error}
            />
          )}
          {step === 2 && pubkey && (
            <BackupStep
              pubkey={pubkey}
              password={password}
              onPasswordChange={setPassword}
              onExport={handleExportBackup}
              backupDone={backupDone}
              onContinue={() => setStep(3)}
              loading={loading}
              error={error}
            />
          )}
          {step === 3 && (
            <BenchmarkStep
              onRun={handleRunBenchmark}
              result={benchmarkResult}
              loading={loading}
              error={error}
            />
          )}
          {step === 4 && (
            <SourceStep
              onSelect={handleSetSource}
              loading={loading}
              error={error}
            />
          )}
          {step === 5 && (
            <GoLiveStep
              sourceType={sourceType}
              onGoLive={handleComplete}
              loading={loading}
              error={error}
            />
          )}
        </div>
      </div>
    </div>
  );
}

// ── Sub-step components ───────────────────────────────────────────────────────

function StepIndicator({ current, labels }: { current: number; labels: string[] }) {
  return (
    <div style={{ display: "flex", gap: 8, marginBottom: 24, alignItems: "center" }}>
      {labels.map((label, i) => {
        const num = i + 1;
        const done = num < current;
        const active = num === current;
        return (
          <div key={num} style={{ display: "flex", alignItems: "center", gap: 4 }}>
            <div
              style={{
                width: 28,
                height: 28,
                borderRadius: "50%",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: 13,
                fontWeight: 600,
                background: done ? COLORS.success : active ? COLORS.accent : COLORS.border,
                color: done || active ? "#fff" : COLORS.muted,
                flexShrink: 0,
              }}
            >
              {done ? "✓" : num}
            </div>
            <span
              style={{
                fontSize: 12,
                color: active ? COLORS.text : COLORS.muted,
                display: i === labels.length - 1 ? "none" : "block",
              }}
            >
              {label}
            </span>
            {i < labels.length - 1 && (
              <div style={{ width: 16, height: 1, background: COLORS.border, flexShrink: 0 }} />
            )}
          </div>
        );
      })}
    </div>
  );
}

function IdentityStep({
  onGenerate,
  loading,
  error,
}: {
  onGenerate: () => void;
  loading: boolean;
  error: string | null;
}) {
  return (
    <div>
      <h2 style={styles.heading}>Criar sua Identidade Prism</h2>
      <p style={styles.body}>
        Sua identidade é um par de chaves criptográficas Ed25519. Ela identifica
        você na rede de forma única — sem e-mail, sem senha, sem servidor.
      </p>
      {error && <ErrorBox msg={error} />}
      <PrimaryButton onClick={onGenerate} disabled={loading}>
        {loading ? "Gerando..." : "Gerar Identidade"}
      </PrimaryButton>
    </div>
  );
}

function BackupStep({
  pubkey,
  password,
  onPasswordChange,
  onExport,
  backupDone,
  onContinue,
  loading,
  error,
}: {
  pubkey: string;
  password: string;
  onPasswordChange: (v: string) => void;
  onExport: () => void;
  backupDone: boolean;
  onContinue: () => void;
  loading: boolean;
  error: string | null;
}) {
  return (
    <div>
      <h2 style={styles.heading}>Backup da Identidade</h2>
      <div style={styles.pubkeyBox}>
        <span style={styles.pubkeyLabel}>Seu endereço Prism:</span>
        <span style={styles.pubkeyValue}>{pubkey}</span>
      </div>
      <p style={styles.warn}>
        ⚠ Sem backup, se reinstalar o Studio, você perderá sua identidade.
      </p>
      {!backupDone ? (
        <>
          <input
            type="password"
            placeholder="Senha para o backup"
            value={password}
            onChange={(e) => onPasswordChange(e.target.value)}
            style={styles.input}
          />
          {error && <ErrorBox msg={error} />}
          <PrimaryButton onClick={onExport} disabled={loading}>
            {loading ? "Exportando..." : "Exportar Backup"}
          </PrimaryButton>
          <GhostButton onClick={onContinue}>Pular por agora</GhostButton>
        </>
      ) : (
        <>
          <p style={{ color: COLORS.success, fontSize: 14, margin: "12px 0" }}>
            ✓ Backup salvo como <code>prism_studio_backup.json</code>
          </p>
          <PrimaryButton onClick={onContinue} disabled={false}>
            Continuar
          </PrimaryButton>
        </>
      )}
    </div>
  );
}

function BenchmarkStep({
  onRun,
  result,
  loading,
  error,
}: {
  onRun: () => void;
  result: string | null;
  loading: boolean;
  error: string | null;
}) {
  return (
    <div>
      <h2 style={styles.heading}>Benchmark de Rede</h2>
      <p style={styles.body}>
        Vamos medir a capacidade do seu sistema para recomendar a melhor qualidade
        de stream.
      </p>
      {error && <ErrorBox msg={error} />}
      {result ? (
        <div style={styles.resultBox}>
          <span style={{ color: COLORS.success, fontSize: 28, fontWeight: 700 }}>
            {result}
          </span>
          <p style={{ color: COLORS.muted, fontSize: 13, margin: "8px 0 0" }}>
            Qualidade recomendada para a sua conexão
          </p>
        </div>
      ) : (
        <PrimaryButton onClick={onRun} disabled={loading}>
          {loading ? "Medindo..." : "Iniciar Benchmark"}
        </PrimaryButton>
      )}
    </div>
  );
}

function SourceStep({
  onSelect,
  loading,
  error,
}: {
  onSelect: (t: "camera" | "rtmp") => void;
  loading: boolean;
  error: string | null;
}) {
  return (
    <div>
      <h2 style={styles.heading}>Fonte de Vídeo</h2>
      <p style={styles.body}>
        Como você vai capturar o vídeo para a live?
      </p>
      {error && <ErrorBox msg={error} />}
      <div style={{ display: "flex", gap: 12, marginTop: 16 }}>
        <SourceCard
          title="Câmera Nativa"
          desc="Webcam conectada ao computador"
          icon="📷"
          onClick={() => onSelect("camera")}
          disabled={loading}
        />
        <SourceCard
          title="OBS / RTMP"
          desc="Enviar stream do OBS para o Studio"
          icon="🎬"
          onClick={() => onSelect("rtmp")}
          disabled={loading}
        />
      </div>
    </div>
  );
}

function GoLiveStep({
  sourceType,
  onGoLive,
  loading,
  error,
}: {
  sourceType: "camera" | "rtmp" | null;
  onGoLive: () => void;
  loading: boolean;
  error: string | null;
}) {
  return (
    <div>
      <h2 style={styles.heading}>Pronto para ir ao Ar!</h2>
      <p style={styles.body}>
        Tudo configurado. Fonte:{" "}
        <strong style={{ color: COLORS.accent }}>
          {sourceType === "rtmp" ? "OBS/RTMP" : "Câmera"}
        </strong>
        .
      </p>
      {error && <ErrorBox msg={error} />}
      <PrimaryButton
        onClick={onGoLive}
        disabled={loading}
        style={{ fontSize: 18, padding: "14px 40px", marginTop: 8 }}
      >
        {loading ? "Iniciando..." : "🔴 IR AO VIVO"}
      </PrimaryButton>
    </div>
  );
}

function TutorialOverlay({ onDismiss }: { onDismiss: () => void }) {
  const tips = [
    { icon: "🎥", title: "Preview", desc: "Acompanhe a prévia do seu stream em tempo real" },
    { icon: "📊", title: "Métricas", desc: "Viewers, bitrate e latência na barra lateral" },
    { icon: "🌐", title: "Rede P2P", desc: "Árvore de relay — seus chunks chegando a todos" },
    { icon: "💰", title: "Ganhos", desc: "State channel: pagamentos por segundo de stream" },
  ];
  return (
    <div style={styles.overlay}>
      <div style={{ ...styles.card, maxWidth: 480 }}>
        <h2 style={{ ...styles.heading, textAlign: "center" }}>As 4 Zonas do Studio</h2>
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, margin: "16px 0" }}>
          {tips.map((t) => (
            <div key={t.title} style={styles.tipCard}>
              <span style={{ fontSize: 28 }}>{t.icon}</span>
              <strong style={{ color: COLORS.text, fontSize: 14 }}>{t.title}</strong>
              <span style={{ color: COLORS.muted, fontSize: 12 }}>{t.desc}</span>
            </div>
          ))}
        </div>
        <PrimaryButton onClick={onDismiss} disabled={false}>
          Começar a usar o Studio
        </PrimaryButton>
      </div>
    </div>
  );
}

// ── Shared small components ──────────────────────────────────────────────────

function SourceCard({
  title,
  desc,
  icon,
  onClick,
  disabled,
}: {
  title: string;
  desc: string;
  icon: string;
  onClick: () => void;
  disabled: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      style={{
        flex: 1,
        padding: 20,
        background: COLORS.card,
        border: `1px solid ${COLORS.border}`,
        borderRadius: 10,
        cursor: disabled ? "not-allowed" : "pointer",
        textAlign: "center",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 8,
      }}
    >
      <span style={{ fontSize: 36 }}>{icon}</span>
      <strong style={{ color: COLORS.text, fontSize: 15 }}>{title}</strong>
      <span style={{ color: COLORS.muted, fontSize: 12 }}>{desc}</span>
    </button>
  );
}

function PrimaryButton({
  children,
  onClick,
  disabled,
  style,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled: boolean;
  style?: React.CSSProperties;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      style={{
        display: "block",
        width: "100%",
        padding: "12px 24px",
        marginTop: 16,
        background: disabled ? COLORS.border : COLORS.accent,
        color: "#fff",
        border: "none",
        borderRadius: 8,
        fontSize: 15,
        fontWeight: 600,
        cursor: disabled ? "not-allowed" : "pointer",
        ...style,
      }}
    >
      {children}
    </button>
  );
}

function GhostButton({
  children,
  onClick,
}: {
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      style={{
        display: "block",
        width: "100%",
        padding: "10px",
        marginTop: 8,
        background: "transparent",
        color: COLORS.muted,
        border: "none",
        fontSize: 13,
        cursor: "pointer",
        textDecoration: "underline",
      }}
    >
      {children}
    </button>
  );
}

function ErrorBox({ msg }: { msg: string }) {
  return (
    <div
      style={{
        padding: "10px 14px",
        background: "#2d2020",
        border: "1px solid #742a2a",
        borderRadius: 6,
        color: "#fc8181",
        fontSize: 13,
        margin: "12px 0",
      }}
    >
      {msg}
    </div>
  );
}

// ── Styles ───────────────────────────────────────────────────────────────────

const styles: Record<string, React.CSSProperties> = {
  overlay: {
    position: "fixed",
    inset: 0,
    background: "rgba(0,0,0,0.85)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 100,
  },
  card: {
    background: COLORS.bg,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 14,
    padding: 32,
    width: "100%",
    maxWidth: 560,
    boxSizing: "border-box",
  },
  content: {
    minHeight: 200,
  },
  heading: {
    color: COLORS.text,
    fontSize: 20,
    fontWeight: 700,
    margin: "0 0 12px",
  },
  body: {
    color: COLORS.muted,
    fontSize: 14,
    lineHeight: 1.6,
    margin: "0 0 16px",
  },
  warn: {
    color: COLORS.warn,
    fontSize: 13,
    margin: "12px 0",
  },
  pubkeyBox: {
    background: COLORS.card,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 8,
    padding: "12px 16px",
    marginBottom: 12,
    display: "flex",
    flexDirection: "column",
    gap: 4,
  },
  pubkeyLabel: {
    color: COLORS.muted,
    fontSize: 11,
    textTransform: "uppercase",
    letterSpacing: "0.05em",
  },
  pubkeyValue: {
    color: COLORS.accent,
    fontFamily: "monospace",
    fontSize: 16,
    fontWeight: 600,
  },
  input: {
    width: "100%",
    padding: "10px 14px",
    background: COLORS.card,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 8,
    color: COLORS.text,
    fontSize: 14,
    boxSizing: "border-box",
    outline: "none",
  },
  resultBox: {
    background: COLORS.card,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 10,
    padding: 24,
    textAlign: "center",
    margin: "16px 0",
  },
  tipCard: {
    background: COLORS.card,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 8,
    padding: 16,
    display: "flex",
    flexDirection: "column",
    gap: 6,
  },
};
