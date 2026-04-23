interface OBSGuideProps {
  onClose: () => void;
}

const COLORS = {
  bg: "#171923",
  card: "#1a2035",
  border: "#2d3748",
  accent: "#4f9cf9",
  success: "#68d391",
  text: "#e2e8f0",
  muted: "#718096",
  code: "#2d3748",
};

const STEPS = [
  {
    num: 1,
    title: "Abrir configurações do OBS",
    desc: 'No OBS Studio, clique em "Configurações" (ícone de engrenagem no canto inferior direito).',
  },
  {
    num: 2,
    title: "Ir para a aba Stream",
    desc: 'Na barra lateral das configurações, clique em "Stream".',
  },
  {
    num: 3,
    title: 'Selecionar "Serviço personalizado"',
    desc: 'No campo "Serviço", escolha "Personalizado..." no menu suspenso.',
  },
  {
    num: 4,
    title: "Inserir URL do servidor",
    desc: "Cole o endereço abaixo no campo Servidor:",
    code: "rtmp://localhost:1935/live",
  },
  {
    num: 5,
    title: "Chave de stream",
    desc: "No campo Chave de stream, use qualquer valor (ex: prism). Não será verificado.",
    code: "prism",
  },
  {
    num: 6,
    title: "Iniciar transmissão no OBS",
    desc: 'Clique em "Iniciar transmissão" no OBS. Em seguida, clique em "IR AO VIVO" no Studio.',
  },
];

export function OBSGuide({ onClose }: OBSGuideProps) {
  return (
    <div style={styles.overlay}>
      <div style={styles.card}>
        <div style={styles.header}>
          <div>
            <h2 style={styles.heading}>Integração com OBS Studio</h2>
            <p style={styles.subtitle}>
              Configure o OBS para enviar o stream via RTMP para o Prism Studio.
            </p>
          </div>
          <button onClick={onClose} style={styles.closeBtn}>
            ✕
          </button>
        </div>

        <div style={styles.stepsContainer}>
          {STEPS.map((step) => (
            <div key={step.num} style={styles.step}>
              <div style={styles.stepNum}>{step.num}</div>
              <div style={styles.stepContent}>
                <strong style={styles.stepTitle}>{step.title}</strong>
                <p style={styles.stepDesc}>{step.desc}</p>
                {step.code && (
                  <div style={styles.codeBlock}>
                    <code style={styles.code}>{step.code}</code>
                    <button
                      style={styles.copyBtn}
                      onClick={() => navigator.clipboard?.writeText(step.code!)}
                    >
                      Copiar
                    </button>
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>

        <div style={styles.infoBox}>
          <span style={{ fontSize: 18 }}>ℹ️</span>
          <span style={{ color: COLORS.muted, fontSize: 13, lineHeight: 1.5 }}>
            Latência adicional de 200–400ms para recodificação AV1. O stream
            chega aos viewers em 7–20 segundos no total.
          </span>
        </div>

        <button onClick={onClose} style={styles.doneBtn}>
          Entendido
        </button>
      </div>
    </div>
  );
}

const styles: Record<string, React.CSSProperties> = {
  overlay: {
    position: "fixed",
    inset: 0,
    background: "rgba(0,0,0,0.8)",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    zIndex: 200,
  },
  card: {
    background: COLORS.bg,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 14,
    padding: 28,
    width: "100%",
    maxWidth: 540,
    maxHeight: "85vh",
    overflowY: "auto",
    boxSizing: "border-box",
  },
  header: {
    display: "flex",
    justifyContent: "space-between",
    alignItems: "flex-start",
    marginBottom: 20,
  },
  heading: {
    color: COLORS.text,
    fontSize: 18,
    fontWeight: 700,
    margin: 0,
  },
  subtitle: {
    color: COLORS.muted,
    fontSize: 13,
    margin: "4px 0 0",
  },
  closeBtn: {
    background: "transparent",
    border: "none",
    color: COLORS.muted,
    fontSize: 18,
    cursor: "pointer",
    padding: 4,
    lineHeight: 1,
    flexShrink: 0,
  },
  stepsContainer: {
    display: "flex",
    flexDirection: "column",
    gap: 16,
    marginBottom: 20,
  },
  step: {
    display: "flex",
    gap: 14,
    alignItems: "flex-start",
  },
  stepNum: {
    width: 28,
    height: 28,
    borderRadius: "50%",
    background: COLORS.accent,
    color: "#fff",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    fontSize: 13,
    fontWeight: 700,
    flexShrink: 0,
  },
  stepContent: {
    flex: 1,
  },
  stepTitle: {
    color: COLORS.text,
    fontSize: 14,
    display: "block",
    marginBottom: 4,
  },
  stepDesc: {
    color: COLORS.muted,
    fontSize: 13,
    margin: "0 0 8px",
    lineHeight: 1.5,
  },
  codeBlock: {
    background: COLORS.code,
    borderRadius: 6,
    padding: "8px 12px",
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 8,
  },
  code: {
    color: COLORS.accent,
    fontFamily: "monospace",
    fontSize: 14,
  },
  copyBtn: {
    background: "transparent",
    border: `1px solid ${COLORS.border}`,
    borderRadius: 4,
    color: COLORS.muted,
    fontSize: 11,
    padding: "2px 8px",
    cursor: "pointer",
    flexShrink: 0,
  },
  infoBox: {
    background: COLORS.card,
    border: `1px solid ${COLORS.border}`,
    borderRadius: 8,
    padding: "12px 14px",
    display: "flex",
    gap: 10,
    alignItems: "flex-start",
    marginBottom: 20,
  },
  doneBtn: {
    display: "block",
    width: "100%",
    padding: "12px",
    background: COLORS.accent,
    color: "#fff",
    border: "none",
    borderRadius: 8,
    fontSize: 15,
    fontWeight: 600,
    cursor: "pointer",
  },
};
