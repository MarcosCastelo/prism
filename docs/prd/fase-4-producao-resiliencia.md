# PRD — Fase 4: Produção e Resiliência

**Projeto:** Prism  
**Fase:** 4 de 4  
**Período:** Meses 11–14 | Sprints 16–18 | Semanas 31–42  
**Pré-requisito:** Fase 3 concluída (chat, identidade, descoberta de streams)  
**Desbloqueia:** Lançamento público  

---

## Objetivo

Tornar o Prism estável para produção: clientes polidos com onboarding completo, pagamentos via state channels off-chain, telemetria distribuída sem servidor central, app mobile nativo, CI/CD multi-plataforma e validação de todos os RNFs sob carga real (10.000 viewers, chaos de 60% de nós).

---

## Modelo de Ameaças — Produção

| Ameaça | Descrição | Mitigação |
|--------|-----------|-----------|
| **Fraude em state channel** | Nó tenta fechar canal com estado antigo (maior saldo a seu favor) | Canal usa sequência monotônica de micro-transações. Fechar canal requer apresentar estado mais recente. Qualquer parte pode contestar fechamento com estado mais novo dentro de um período de disputa (7 dias on-chain). |
| **Double-spend em micropagamento** | Nó aceita pagamento e então nega ter recebido | Cada micropagamento é um estado assinado por ambas as partes. Nó receptor armazena cada estado assinado. Evidência criptográfica impede negação. |
| **DoS na fase de carga** | Atacante inunda a rede durante spike de 10.000 viewers | Rate limiting em todas as camadas (IP, pubkey, por stream). Nós com health_score baixo são automaticamente evitados pelo roteamento DHT. |
| **Atualização maliciosa de cliente** | Build do Studio/Player comprometido na supply chain | Binários assinados com chave do projeto (codesigning). Checksums publicados separadamente. Usuário pode verificar integridade antes de instalar. |
| **Extração de chave do Secure Enclave (mobile)** | Ataque físico ao dispositivo | Chave privada nunca sai do Secure Enclave (iOS) ou Android Keystore. Operações de assinatura são delegadas ao enclave — a chave em bytes nunca é acessível ao código da aplicação. |
| **Telemetria usada para deanonimização** | Métricas coletadas revelam quem está assistindo o quê | Telemetria é agregada com k-anonymity ≥ 10 antes de propagar. Nenhum dado individual de viewer é transmitido — apenas contadores agregados por stream e região. |

---

## Estrutura de Arquivos

Adicionar ao workspace das Fases 1–3:

```
prism/
└── crates/
    ├── prism-payments/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── channel.rs          # abrir/fechar state channel
    │       ├── micropayment.rs     # estado assinado por segundo de stream
    │       └── settlement.rs       # liquidação on-chain (interface genérica)
    ├── prism-telemetry/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── collector.rs        # coleta métricas locais do nó
    │       └── aggregator.rs       # agrega via gossip — sem servidor central
    ├── prism-studio/ (extensões)
    │   ├── src-tauri/src/
    │   │   ├── onboarding.rs       # fluxo de 5 passos de primeiro uso
    │   │   └── rtmp_server.rs      # servidor RTMP porta 1935 (recebe OBS)
    │   └── src/components/
    │       ├── Onboarding.tsx      # wizard de onboarding
    │       ├── NodeTree.tsx        # visualização da árvore de relay
    │       ├── EarningsPanel.tsx   # state channel: earnings por sessão
    │       └── OBSGuide.tsx        # guia de integração com OBS
    └── prism-player-mobile/        # React Native app
        ├── package.json
        ├── app.json                # Expo config
        └── src/
            ├── App.tsx
            ├── screens/
            │   ├── DiscoverScreen.tsx
            │   ├── StreamScreen.tsx
            │   └── IdentityScreen.tsx
            └── modules/
                ├── HlsPlayer.ts    # expo-av (iOS AVPlayer) + ExoPlayer (Android)
                ├── SecureKey.ts    # Secure Enclave (iOS) / Keystore (Android)
                └── Notifications.ts # push local via DHT polling
```

---

## State Channels — `prism-payments`

### Conceito

State channels permitem micropagamentos off-chain entre viewers/nós sem transação blockchain por segundo. O canal é aberto on-chain uma vez, pagamentos acontecem off-chain a cada segundo de stream entregue, e o canal é fechado on-chain com o estado final.

### `prism-payments/src/channel.rs`

```rust
pub struct ChannelState {
    pub channel_id:       [u8; 32],    // SHA-256(payer_pubkey || payee_pubkey || open_ts)
    pub payer_pubkey:     [u8; 32],
    pub payee_pubkey:     [u8; 32],
    pub sequence:         u64,          // monotônico — apenas estados com seq maior são válidos
    pub payer_balance:    u64,          // saldo restante do pagador (em unidade mínima)
    pub payee_balance:    u64,          // acumulado pelo recebedor
    pub payer_signature:  [u8; 64],    // Ed25519Sign(payer_privkey, hash(state sem sigs))
    pub payee_signature:  [u8; 64],    // Ed25519Sign(payee_privkey, hash(state sem sigs))
}

pub struct StateChannel {
    state:    ChannelState,
    identity: Arc<Identity>,
}

impl StateChannel {
    /// Abrir canal: retorna estado inicial assinado por ambas as partes.
    /// Persiste localmente. Registro on-chain é responsabilidade do caller.
    pub fn open(
        payer: Arc<Identity>,
        payee_pubkey: [u8; 32],
        initial_balance: u64,
    ) -> Self;
    
    /// Aplicar micropagamento: incrementa seq, transfere `amount` do payer para payee.
    /// Retorna novo ChannelState assinado pelo pagador — payee deve contra-assinar.
    pub fn pay(&mut self, amount: u64) -> anyhow::Result<ChannelState>;
    
    /// Contra-assinar estado recebido do pagador (lado do payee).
    /// Verifica: seq > current_seq, saldo consistente, assinatura do payer válida.
    pub fn countersign(&mut self, state: ChannelState) -> anyhow::Result<ChannelState>;
    
    /// Retornar estado mais recente (para fechar o canal).
    pub fn latest_state(&self) -> &ChannelState;
}
```

**Regras de validação de estado:**
```
1. state.sequence > current_state.sequence          — obrigatório (sem replay)
2. state.payer_balance + state.payee_balance == initial_balance  — conservação de valor
3. state.payer_balance <= current_state.payer_balance  — saldo só pode diminuir para o payer
4. Ed25519 signature do payer é válida sobre hash(campos 1-6 do ChannelState)
```

### `prism-payments/src/micropayment.rs`

```rust
pub struct MicropaymentScheduler {
    channel:       Arc<Mutex<StateChannel>>,
    rate_per_sec:  u64,   // unidades por segundo de stream entregue
}

impl MicropaymentScheduler {
    /// Inicia loop de pagamento: a cada segundo de stream entregue, chama channel.pay().
    /// Se payee não contra-assinar em 5s: pausar pagamentos e logar aviso.
    pub async fn start(&self, stream_id: &str);
    
    /// Parar pagamentos e retornar estado final para liquidação.
    pub async fn stop(&self) -> ChannelState;
}
```

### `prism-payments/src/settlement.rs`

```rust
/// Interface genérica para liquidação on-chain.
/// A implementação concreta depende da blockchain escolhida (fora do escopo desta fase).
/// Esta fase apenas define a interface e um mock para testes.
pub trait OnChainSettlement {
    /// Submete estado final do canal para liquidação.
    async fn settle(&self, final_state: ChannelState) -> anyhow::Result<String>; // tx_hash
    
    /// Verifica se um canal está aberto on-chain.
    async fn is_channel_open(&self, channel_id: [u8; 32]) -> anyhow::Result<bool>;
}

pub struct MockSettlement; // implementação de teste — apenas loga, não faz tx real

impl OnChainSettlement for MockSettlement {
    async fn settle(&self, state: ChannelState) -> anyhow::Result<String> {
        tracing::info!("mock settle: channel={} seq={} payee_earned={}",
            hex::encode(state.channel_id), state.sequence, state.payee_balance);
        Ok(hex::encode(sha256(&state.channel_id)))
    }
    
    async fn is_channel_open(&self, _: [u8; 32]) -> anyhow::Result<bool> { Ok(true) }
}
```

---

## Telemetria Distribuída — `prism-telemetry`

### `prism-telemetry/src/collector.rs`

```rust
#[derive(serde::Serialize, Clone)]
pub struct NodeMetrics {
    pub node_id:          String,    // primeiros 8 chars do node_id hex
    pub region:           String,
    pub capacity_class:   String,
    pub streams_serving:  u32,
    pub viewers_served:   u32,       // estimativa
    pub uptime_s:         u64,
    pub cpu_usage_pct:    f32,
    pub bandwidth_mbps:   f32,
    pub health_score:     f32,
    pub collected_at_ms:  u64,
}

impl NodeMetrics {
    /// Coleta métricas locais. Chamado a cada 30s.
    pub fn collect_local() -> Self;
}
```

### `prism-telemetry/src/aggregator.rs`

```rust
/// Agrega métricas da rede via gossip (libp2p gossipsub topic "prism/telemetry").
/// k-anonymity: só propaga se pelo menos 10 nós contribuíram para o agregado.
pub struct TelemetryAggregator {
    received: DashMap<String, NodeMetrics>,  // node_id → métricas mais recentes
}

impl TelemetryAggregator {
    pub fn on_metrics_received(&self, metrics: NodeMetrics);
    
    /// Retorna métricas agregadas da rede. Nunca expõe dados individuais.
    pub fn network_summary(&self) -> NetworkSummary;
}

#[derive(serde::Serialize)]
pub struct NetworkSummary {
    pub total_nodes:       u32,
    pub total_streams:     u32,
    pub total_viewers:     u32,   // soma das estimativas
    pub avg_health_score:  f32,
    pub nodes_by_class:    HashMap<String, u32>,
    pub nodes_by_region:   HashMap<String, u32>,
    pub summary_at_ms:     u64,
}
```

---

## Studio Polido — Tauri 2

### Onboarding (5 Passos)

**Requisito:** Streamer deve concluir em < 5 minutos sem conhecimento técnico de P2P.

```
Passo 1 — Instalação:
  - Executável único sem dependências externas
  - Windows: .msi; macOS: .dmg com codesigning; Linux: AppImage
  - Verificar hash do instalador exibido na tela de download

Passo 2 — Identidade:
  - Geração automática de par Ed25519 na primeira execução
  - Exibir pubkey como "Seu endereço Prism: pr1a3f7...b92e" (não "Ed25519 pubkey")
  - Oferecer backup imediato: arquivo .json criptografado por senha
  - Aviso: "Sem backup, se reinstalar o Studio, perde sua identidade"

Passo 3 — Benchmark de rede:
  - Executar run_benchmark() com UI de progresso
  - Exibir resultado: "Sua conexão suporta stream em 1080p60" ou "720p" etc.
  - Configurar prism-node como relay em background automaticamente

Passo 4 — Fonte de vídeo:
  - Wizard: câmera nativa OU OBS via RTMP
  - Se OBS: exibir instrução passo-a-passo com screenshot:
    "Settings → Stream → Custom → Server: rtmp://localhost:1935/live"
  - Testar fonte antes de avançar (preview)

Passo 5 — GO LIVE:
  - Botão principal com tooltip overlay (pode ser dispensado)
  - Primeira live: exibir tutorial de 30s sobre as 4 zonas da interface
```

### Servidor RTMP — `prism-studio/src-tauri/src/rtmp_server.rs`

```rust
/// Inicia servidor RTMP na porta 1935 em localhost.
/// Recebe stream H.264/H.265 do OBS e repassa para prism-ingest/injector.
/// Latência adicional do re-encode: 200–400ms (dentro da janela de 7–20s).
pub struct RtmpServer { /* ... */ }

impl RtmpServer {
    pub async fn start(port: u16 /* = 1935 */) -> anyhow::Result<Self>;
    pub async fn stop(&self);
    
    /// Stream de frames recebidos do OBS para o encoder AV1.
    pub fn frame_stream(&self) -> impl Stream<Item = RawFrame>;
}
```

### Componentes React do Studio Polido

```typescript
// NodeTree.tsx
// Visualiza a árvore de relay em tempo real
// Cada nó: ponto colorido (verde/amarelo/vermelho) + hover com métricas
// Atualizado a cada 5s via evento Tauri "node-tree-update"
interface NodeTreeProps {
  onNodeClick: (nodeId: string) => void;
}

// EarningsPanel.tsx
// Exibe earnings do state channel: por sessão, acumulado, status do canal
// Botão "Fechar canal e liquidar"
interface EarningsPanelProps {
  channelState: ChannelState | null;
}
```

---

## Player Mobile — React Native

### `prism-player-mobile/package.json`
```json
{
  "name": "prism-player-mobile",
  "version": "0.1.0",
  "main": "node_modules/expo/AppEntry.js",
  "dependencies": {
    "expo": "~51",
    "expo-av": "~14",
    "expo-secure-store": "~13",
    "expo-notifications": "~0.28",
    "react": "18",
    "react-native": "0.74",
    "@react-navigation/native": "^6",
    "@react-navigation/stack": "^6",
    "react-native-screens": "^3",
    "@noble/ed25519": "^2"
  }
}
```

### `prism-player-mobile/src/modules/HlsPlayer.ts`

```typescript
// iOS: usa AVPlayer nativo via expo-av — suporte AV1 em dispositivos com chip A17+
// Android: usa ExoPlayer — suporte AV1 software em Android 10+, hardware em Android 12+
// Fallback H.264 para dispositivos sem suporte AV1

import { Video, ResizeMode } from 'expo-av';

export interface PlayerState {
  currentQuality: string;  // "360p" | "480p" | "720p" | "1080p"
  isBuffering: boolean;
  latencyEstimate: number; // ms
}
```

### `prism-player-mobile/src/modules/SecureKey.ts`

```typescript
import * as SecureStore from 'expo-secure-store';

// iOS: armazena chave privada no Keychain com proteção kSecAttrAccessibleWhenUnlocked
// Android: armazena no Android Keystore system
// A chave em bytes NUNCA é exposta ao código da aplicação após armazenamento

export async function storePrivateKey(key: Uint8Array): Promise<void>;
export async function signWithStoredKey(data: Uint8Array): Promise<Uint8Array>;
export async function hasStoredKey(): Promise<boolean>;
export async function deleteStoredKey(): Promise<void>;
```

### `prism-player-mobile/src/modules/Notifications.ts`

```typescript
// Push notifications sem servidor central
// Implementação: polling da DHT a cada 60s para streams de streamers seguidos
// Se streamer iniciar live: dispara notificação local via Expo Notifications
// Sem Firebase/APNs server — puramente local

export async function scheduleLocalNotification(
  streamerId: string,
  streamerName: string,
  streamTitle: string,
): Promise<void>;

export async function startFollowPolling(
  followedStreamers: string[],  // array de pubkey_hex
  intervalMs: number,           // default: 60_000
): Promise<void>;
```

---

## CI/CD — GitHub Actions

### `.github/workflows/build.yml`

```yaml
name: Build Prism

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: sudo apt-get install -y libsvtav1-dev coturn
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings

  build-studio:
    needs: test
    strategy:
      matrix:
        platform: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.platform }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with: { node-version: '20' }
      - run: cd crates/prism-studio && npm install
      - uses: tauri-apps/tauri-action@v0
        with:
          projectPath: crates/prism-studio
          tauriScript: cargo tauri

  build-mobile:
    needs: test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: '20' }
      - run: cd crates/prism-player-mobile && npm install
      - run: npx expo export  # web build; iOS/Android via EAS Build (externo)
```

---

## Testes de Carga e Caos

### Cenário 1 — 10.000 viewers simultâneos

```
Setup:
  - Testnet de 50 nós em VMs distribuídas (mín 3 regiões geográficas)
  - 1 streamer transmitindo 1080p60
  - 10.000 viewers simulados com k6 ou Locust fazendo requests HLS

Métricas a medir:
  - Latência streamer→viewer P50, P95, P99
  - Taxa de frame loss
  - CPU e memória por classe de nó
  - Tempo de join de novo viewer (até primeiro frame)

PASSA se:
  - P95 latência < 20s
  - Frame loss < 0,5% em condições estáveis
  - Nenhum nó com CPU > 90% sustentado por mais de 30s
  - Novo viewer recebe primeiro frame em < 10s
```

### Cenário 2 — Chaos: 60% dos nós caindo

```
Setup:
  - Testnet de 30 nós com stream ativa
  - 500 viewers ativos

Procedimento:
  - Matar 18 nós aleatoriamente (60%) via SIGKILL simultâneo
  - Monitorar stream nos 500 viewers por 5 minutos adicionais

PASSA se:
  - Stream não interrompe (máx 2s de buffer vazio permitido durante failover)
  - Todos os viewers reconectam automaticamente em < 30s
  - Chat continua funcionando (gossip sobrevive com 40% dos nós)
  - Nenhum viewer precisa de ação manual para reconectar
```

### Cenário 3 — Ataque de Sybil em produção

```
Setup:
  - Rede de 20 nós honestos + 10 nós maliciosos
  - Nós maliciosos enviam chunks com streamer_sig inválida

PASSA se:
  - Nenhum chunk inválido é entregue ao viewer
  - Nós maliciosos são banidos pelo sistema de reputação em < 60s
  - Rede continua funcionando normalmente após ban dos nós maliciosos
```

### Cenário 4 — Fraud em state channel

```
Procedimento:
  - Abrir canal entre payer e payee
  - Realizar 100 micropagamentos (estado seq=100)
  - Payee tenta fechar canal com estado seq=50 (fraude)

PASSA se:
  - Sistema detecta que o estado submetido não é o mais recente
  - Payer pode apresentar estado seq=100 como contestação
  - Canal é liquidado com o estado correto (seq=100)
```

---

## Dependências Cargo

### `prism-payments/Cargo.toml`
```toml
[package]
name = "prism-payments"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core = { path = "../prism-core" }
tokio      = { version = "1", features = ["full"] }
anyhow     = "1"
thiserror  = "1"
tracing    = "0.1"
serde      = { version = "1", features = ["derive"] }
hex        = "0.4"
```

### `prism-telemetry/Cargo.toml`
```toml
[package]
name = "prism-telemetry"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core = { path = "../prism-core" }
libp2p     = { version = "0.54", features = ["gossipsub", "tokio"] }
dashmap    = "5"
tokio      = { version = "1", features = ["full"] }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow     = "1"
tracing    = "0.1"
```

---

## Ordem de Implementação

```
Sprint 16 (Semanas 31–36): State Channels + Telemetria
1. prism-payments/src/channel.rs           — open, pay, countersign, latest_state
2. prism-payments/src/micropayment.rs      — scheduler 1 pagamento/s
3. prism-payments/src/settlement.rs        — interface + MockSettlement
4. prism-telemetry/src/collector.rs        — métricas locais
5. prism-telemetry/src/aggregator.rs       — gossip + NetworkSummary
   Teste: ciclo completo open→pay(100x)→settle com MockSettlement
   Teste: fraude em state channel detectada

Sprint 17 (Semanas 37–40): Studio e Player Polidos
6. prism-studio: onboarding 5 passos (Onboarding.tsx + onboarding.rs)
7. prism-studio: servidor RTMP (rtmp_server.rs + OBSGuide.tsx)
8. prism-studio: NodeTree.tsx com dados reais de telemetria
9. prism-studio: EarningsPanel.tsx integrado ao state channel
10. prism-player-mobile: HlsPlayer.ts + SecureKey.ts + Notifications.ts
11. prism-player-mobile: DiscoverScreen + StreamScreen + IdentityScreen
    Teste: streamer inicia live em < 5 minutos do zero (onboarding completo)
    Teste: viewer assiste em < 30s sem configuração manual

Sprint 18 (Semanas 41–42): Carga, Caos e CI/CD
12. .github/workflows/build.yml — CI com cargo test + clippy + tauri-action
13. Executar Cenário 1 (10.000 viewers)
14. Executar Cenário 2 (60% dos nós caem)
15. Executar Cenário 3 (ataque Sybil)
16. Executar Cenário 4 (fraude state channel)
    Corrigir qualquer falha descoberta nos cenários
```

---

## Critérios de Conclusão da Fase (e do Projeto)

| # | Critério | Verificação |
|---|---------|-------------|
| C1 | Ciclo completo state channel: open → 100 pagamentos → settle | Teste unitário |
| C2 | Fraude em state channel detectada e revertida | Cenário 4 |
| C3 | Streamer inicia primeira live em < 5 min (onboarding completo) | Teste manual cronometrado |
| C4 | Viewer assiste stream em < 30s sem configuração manual | Teste manual |
| C5 | 10.000 viewers: latência P95 < 20s, frame loss < 0,5% | Cenário 1 |
| C6 | 60% dos nós caem: stream sobrevive, viewers reconectam em < 30s | Cenário 2 |
| C7 | Nós maliciosos banidos em < 60s após envio de chunks inválidos | Cenário 3 |
| C8 | Telemetria não expõe dados individuais de viewers | Auditoria de código |
| C9 | CI/CD: build do Studio para Win/Mac/Linux + Player web | GitHub Actions verde |
| C10 | App mobile: HLS funcional em iOS 17+ e Android 10+ | Teste em dispositivos reais |
| C11 | Todos os RNFs do PRD global validados (latência, disponibilidade, segurança) | Relatório de testes |

---

## RNFs Globais do Projeto (validar nesta fase)

| ID | Métrica | Alvo | Cenário de teste |
|----|---------|------|-----------------|
| RNF-01 | Latência vídeo P95 | < 20s | Cenário 1 |
| RNF-02 | Latência chat P95 | < 500ms | Teste de cobertura gossip (Fase 3) |
| RNF-03 | Disponibilidade com 60% de nós caindo | Stream não para | Cenário 2 |
| RNF-04 | Escalabilidade | Latência estável de 10 para 10.000 viewers | Cenário 1 |
| RNF-05 | Chunks inválidos rejeitados | 100% | Cenário 3 + testes Fase 2 |
| RNF-06 | Compatibilidade HLS | Chrome, Firefox, Safari, VLC | Teste manual Fase 2 |
