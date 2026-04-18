# PRD — Fase 2: Pipeline de Vídeo

**Projeto:** Prism  
**Fase:** 2 de 4  
**Período:** Meses 4–7 | Sprints 5–12 | Semanas 9–24  
**Pré-requisito:** Fase 1 concluída (DHT, QUIC, identidade, reputação)  
**Desbloqueia:** Fases 3 e 4  

---

## Objetivo

Implementar o pipeline completo de streaming de vídeo: encode AV1/SVC no streamer → injeção em 4 seed nodes via DHT → topologia de árvore com failover QUIC → classes de nós (A/B/C/edge) → entrega HLS ao viewer. Latência streamer→viewer P95 < 20s.

Inclui a UI básica do **Studio** (Tauri 2) e do **Player** (PWA React).

---

## Modelo de Ameaças — Vídeo

| Ameaça | Descrição | Mitigação |
|--------|-----------|-----------|
| **Injeção de chunk falso** | Relay malicioso injeta chunk com conteúdo diferente | Todo chunk carrega `streamer_sig = Ed25519Sign(streamer_privkey, SHA-256(payload))`. Receptor verifica contra a `streamer_pubkey` conhecida (obtida do stream_record na DHT). Chunk sem assinatura válida do streamer é descartado + peer penalizado. |
| **Hijack de stream** | Nó anuncia stream_id de outro streamer e serve seus próprios chunks | `stream_id = SHA-256(streamer_pubkey \|\| start_ts)` — está matematicamente vinculado à chave pública. Para gerar o mesmo stream_id, o atacante precisaria da chave privada do streamer. Chunks do atacante não terão `streamer_sig` válida. |
| **Replay de chunk antigo** | Relay reenvia chunk antigo para atrasar o stream | `VideoChunk.sequence` é monotônico. Receptor rejeita qualquer chunk com `sequence <= last_seen_sequence` para aquele stream. `prev_chunk_hash` encadeia os chunks — impossível inserir no meio da sequência. |
| **Manifest HLS falso** | Nó Classe A serve manifest apontando para edge nodes maliciosos | Manifests são assinados pelo nó Classe A com sua chave Ed25519. Viewer verifica a assinatura do manifest antes de fazer requests de segmentos. |
| **Layer stripping malicioso** | Edge node remove camadas SVC corretas e serve qualidade degradada falsa** | Layer stripping é detectável: o hash do chunk original (com todas as layers) é assinado pelo streamer. Se o edge falsificar que o chunk tem apenas L0 quando tem L3, o viewer detecta inconsistência ao verificar o hash da layer base contra o hash total. **Nota:** na prática, layer stripping legítimo produz payload diferente — a verificação é do hash da layer stripping esperada, não do chunk completo. Implementar `LayerHash` por camada em cada `VideoChunk`. |
| **Decoder exploit** | Payload AV1 malicioso explora vulnerabilidade no decoder do viewer | Viewer executa o decoder AV1 (dav1d) em processo isolado com seccomp no Linux / sandbox no macOS. Falha no decoder não propaga para o processo principal. |
| **Flood de NACK** | Nó malicioso envia NACKs excessivos para saturar upstream | Rate limiting: máx 5 NACKs pendentes por stream por peer. NACK ignorado se `deadline_ms` já passou. Peer com NACKs excessivos recebe penalidade de reputação. |
| **Chunk com sequence gap artificial** | Relay descarta chunks para forçar NACK storm | Detectado por monitoramento: se gap rate de um peer específico > 10%, reduzir health_score e evitar roteamento por esse peer. |

---

## Estrutura de Arquivos

Adicionar ao workspace da Fase 1:

```
prism/
├── proto/
│   ├── video_chunk.proto           # chunk AV1/SVC assinado pelo streamer
│   ├── hls_manifest.proto          # manifest HLS assinado pelo nó Classe A
│   ├── rs_fragment.proto           # fragmento Reed-Solomon
│   └── nack_request.proto          # pedido de retransmissão
├── crates/
│   ├── prism-encoder/              # SVT-AV1 FFI + segmentador fMP4
│   │   ├── Cargo.toml
│   │   ├── build.rs                # bindgen SVT-AV1
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── svt_av1.rs          # FFI wrapper: EbComponentType, encode_frame
│   │       ├── svc_layers.rs       # gerenciamento de layers L0–L3
│   │       └── segmenter.rs        # acumula frames → fMP4 de 3s
│   ├── prism-ingest/               # lado do streamer: captura + encode + injeção
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── capture.rs          # A/V capture (câmera, tela, RTMP input)
│   │       ├── rtmp_server.rs      # servidor RTMP local para receber do OBS
│   │       └── injector.rs         # sign chunk + inject em 4 seeds via DHT
│   ├── prism-node/                 # extensões ao nó da Fase 1
│   │   └── src/
│   │       ├── relay/
│   │       │   ├── mod.rs
│   │       │   ├── tree.rs         # topologia fanout=8, dual-parent QUIC
│   │       │   └── failover.rs     # ativação do backup quando primary cai
│   │       ├── classes/
│   │       │   ├── mod.rs
│   │       │   ├── class_a.rs      # remux para HLS, gerar manifests assinados
│   │       │   ├── class_b.rs      # transcode L0→H.264, thumbnails, RS split
│   │       │   ├── class_c.rs      # RS storage, cache, servir late-joiners
│   │       │   └── edge.rs         # layer stripping, servir HLS via HTTP (axum)
│   │       ├── erasure/
│   │       │   └── reed_solomon.rs # wrapper RS(10,4) e RS(4,2) — reed-solomon-erasure
│   │       └── sync/
│   │           ├── playout.rs      # NTP + playout buffer + deadline global
│   │           └── nack.rs         # detector de gap, envio/recebimento de NACK
│   ├── prism-studio/               # app desktop do streamer (Tauri 2)
│   │   ├── src-tauri/
│   │   │   ├── Cargo.toml
│   │   │   ├── tauri.conf.json
│   │   │   └── src/
│   │   │       ├── main.rs
│   │   │       ├── commands.rs     # start_stream, stop_stream, get_metrics
│   │   │       └── events.rs       # emit metrics ao frontend via Tauri events
│   │   └── src/                    # React + TypeScript
│   │       ├── App.tsx
│   │       ├── main.tsx
│   │       └── components/
│   │           ├── Preview.tsx     # preview de câmera/tela a 15fps
│   │           ├── NetworkHealth.tsx  # nós, viewers, bitrate, latência
│   │           └── EncodeSettings.tsx
│   └── prism-player/               # PWA viewer
│       ├── package.json
│       ├── vite.config.ts
│       └── src/
│           ├── App.tsx
│           ├── main.tsx
│           └── player/
│               └── HlsPlayer.tsx   # HLS.js + ABR automático
```

---

## Schemas Protobuf

### `proto/video_chunk.proto`
```protobuf
syntax = "proto3";
package prism;

message LayerHash {
  uint32 layer_index  = 1; // 0=L0, 1=L1, 2=L2, 3=L3
  bytes  payload_hash = 2; // SHA-256 da camada isolada após strip
}

message VideoChunk {
  string stream_id        = 1; // SHA-256(streamer_pubkey || start_ts_ms) — hex
  uint64 sequence         = 2; // monotônico, começa em 1
  uint64 timestamp_ms     = 3; // wall clock do início do segmento
  bytes  payload          = 4; // fMP4 com AV1/SVC, todas as layers embutidas
  bytes  streamer_pubkey  = 5; // Ed25519 pubkey do streamer (32 bytes)
  bytes  streamer_sig     = 6; // Ed25519Sign(streamer_privkey, SHA-256(payload))
  bytes  prev_chunk_hash  = 7; // SHA-256(chunk anterior.payload) — cadeia de chunks
  repeated LayerHash layer_hashes = 8; // hash por layer para verificar strip
}
```

### `proto/hls_manifest.proto`
```protobuf
syntax = "proto3";
package prism;

message HlsManifest {
  string stream_id      = 1;
  uint64 sequence       = 2;  // corresponde ao VideoChunk.sequence
  bytes  master_m3u8    = 3;  // conteúdo do manifest master
  bytes  media_m3u8     = 4;  // conteúdo do manifest de media
  bytes  node_pubkey    = 5;  // pubkey do nó Classe A que gerou
  bytes  node_sig       = 6;  // Ed25519Sign(node_privkey, SHA-256(master||media))
}
```

### `proto/rs_fragment.proto`
```protobuf
syntax = "proto3";
package prism;

message RsFragment {
  string stream_id    = 1;
  uint64 chunk_seq    = 2;
  uint32 frag_index   = 3; // 0–13 para RS(10,4); 0–5 para RS(4,2)
  uint32 total_frags  = 4; // 14 para RS(10,4); 6 para RS(4,2)
  uint32 data_frags   = 5; // 10 para RS(10,4); 4 para RS(4,2)
  bytes  fragment     = 6; // dados do fragmento (tamanho = ceil(payload/data_frags))
  bytes  chunk_hash   = 7; // SHA-256(VideoChunk.payload) — para verificar reconstrução
}
```

### `proto/nack_request.proto`
```protobuf
syntax = "proto3";
package prism;

message NackRequest {
  string stream_id      = 1;
  uint64 missing_seq    = 2;
  uint64 deadline_ms    = 3; // descarta retransmissão se recebida após este instante
  bytes  requester_id   = 4; // node_id do nó que pede a retransmissão
  bytes  requester_sig  = 5; // Ed25519Sign(privkey, hash(stream_id||missing_seq||deadline_ms))
}
```

---

## Especificações Técnicas por Componente

### AV1/SVC — Estrutura de Layers

| Layer | Resolução | FPS | Bitrate | Tipo |
|-------|-----------|-----|---------|------|
| L0 (Base) | 360p | 30 | 400 kbps | Obrigatória — sempre presente |
| L1 | 480p | 30 | +600 kbps | Opcional |
| L2 | 720p | 60 | +1.500 kbps | Opcional |
| L3 | 1080p | 60 | +3.000 kbps | Opcional |

Layer stripping: operação bitwise no bitstream fMP4. Não requer decodificação.  
**Implementar `layer_hashes` em `VideoChunk`:** cada layer tem seu próprio SHA-256 para permitir verificação de integridade após strip sem precisar do chunk original completo.

### `prism-encoder/src/svt_av1.rs`

```rust
// FFI wrapper para libsvtav1 (C library)
// Usar bindgen no build.rs para gerar bindings de EbSvtAv1EncConfiguration

pub struct SvtAv1Encoder {
    // handle para EbComponentType
}

pub struct EncodeConfig {
    pub width:   u32,
    pub height:  u32,
    pub fps:     u32,         // 30 ou 60
    pub bitrate_kbps: u32,    // bitrate total de todas as layers
    pub svc_layers:   u8,     // número de layers SVC: 1–4
    pub low_delay:    bool,   // true para live streaming
    pub preset:       i8,     // 9–12 para real-time (SVT-AV1: quanto maior, mais rápido)
}

impl SvtAv1Encoder {
    pub fn new(config: EncodeConfig) -> anyhow::Result<Self>;
    
    /// Alimenta um frame YUV420 bruto. Retorna packets encodados (pode ser vazio).
    pub fn encode_frame(&mut self, frame: &[u8], pts: u64) -> anyhow::Result<Vec<Vec<u8>>>;
    
    /// Flush: retorna packets restantes no buffer do encoder.
    pub fn flush(&mut self) -> anyhow::Result<Vec<Vec<u8>>>;
}
```

**Integração no `build.rs`:**
```rust
// build.rs de prism-encoder
fn main() {
    println!("cargo:rustc-link-lib=SvtAv1Enc");
    println!("cargo:rustc-link-search=native=/usr/local/lib");
    bindgen::Builder::default()
        .header("wrapper.h")  // inclui EbSvtAv1Enc.h
        .generate()
        .unwrap()
        .write_to_file(out_dir.join("svt_av1_bindings.rs"))
        .unwrap();
}
```

### `prism-encoder/src/segmenter.rs`

```rust
pub struct FMp4Segmenter {
    // acumula packets AV1 encodados até completar 3s de duração
}

pub struct Segment {
    pub data:         Vec<u8>,     // fMP4 completo
    pub duration_ms:  u64,         // duração efetiva (próximo de 3000)
    pub pts_start:    u64,
}

impl FMp4Segmenter {
    pub fn new(target_duration_ms: u64 /* = 3_000 */) -> Self;
    
    /// Alimenta packets do encoder. Retorna Some(Segment) quando 3s completos.
    pub fn push_packet(&mut self, packet: Vec<u8>, pts: u64) -> Option<Segment>;
    
    /// Força fechamento do segmento atual (fim de stream).
    pub fn flush(&mut self) -> Option<Segment>;
}
```

### `prism-ingest/src/injector.rs`

```rust
pub struct StreamInjector {
    identity:   Arc<Identity>,
    stream_id:  String,            // SHA-256(streamer_pubkey || start_ts_ms) hex
    sequence:   AtomicU64,
    prev_hash:  Mutex<[u8; 32]>,   // hash do chunk anterior
}

impl StreamInjector {
    pub fn new(identity: Arc<Identity>, start_ts_ms: u64) -> Self;
    
    /// Recebe Segment, constrói VideoChunk (com assinatura e prev_hash),
    /// serializa e envia para os 4 seed nodes descobertos via DHT.
    /// Retorna Err se menos de 1 seed aceitar o chunk.
    pub async fn inject(&self, segment: Segment, kad: &Kademlia) -> anyhow::Result<()>;
}
```

**Construção de VideoChunk:**
```
stream_id      = hex(sha256(streamer_pubkey || start_ts_ms_bytes))
sequence       = atomic_fetch_add(1)
timestamp_ms   = now_unix_ms()
payload        = segment.data
streamer_sig   = identity.sign(&sha256(&payload))
prev_chunk_hash = *prev_hash (atualizar com sha256(&payload) após construir)
layer_hashes   = [LayerHash { layer_index: i, payload_hash: sha256(strip_to_layer(payload, i)) } for i in 0..n_layers]
```

**Política de injeção:**
- Descobrir seeds via DHT: FIND_VALUE(stream_id + ":seeds")
- Enviar chunk para 4 seeds simultaneamente (tokio::join_all)
- Se seed não responder em 2s: marcar como indisponível, buscar substituto na DHT
- Se < 2 seeds aceitarem: retornar Err (stream não pode continuar com < 2 seeds)

### Reed-Solomon — `prism-node/src/erasure/reed_solomon.rs`

```rust
// Usa crate reed-solomon-erasure = "6" (implementação Backblaze, SIMD-optimized)

pub enum RsConfig {
    /// RS(10, 4): 10 fragmentos de dados + 4 de paridade = 14 total
    /// Necessário: quaisquer 10 dos 14 para reconstruir.
    /// Tolera: perda de até 4 fragmentos.
    Standard,
    
    /// RS(4, 2): 4 fragmentos de dados + 2 de paridade = 6 total
    /// Necessário: quaisquer 4 dos 6 para reconstruir.
    /// Tolera: perda de até 2 fragmentos.
    /// Usar quando rede tem < 10 nós Classe C disponíveis.
    Reduced,
}

pub struct ReedSolomonCoder {
    config: RsConfig,
}

impl ReedSolomonCoder {
    pub fn new(config: RsConfig) -> Self;
    
    /// Divide payload em fragmentos. Retorna Vec<Vec<u8>> de tamanho total_frags.
    /// Primeiros data_frags são dados; últimos parity_frags são paridade.
    pub fn encode(&self, payload: &[u8]) -> anyhow::Result<Vec<Vec<u8>>>;
    
    /// Reconstrói payload a partir de quaisquer data_frags fragmentos dos total_frags.
    /// `fragments`: Vec de Option<Vec<u8>> — None onde fragmento está ausente.
    /// Retorna Err se fragmentos insuficientes para reconstrução.
    pub fn reconstruct(&self, fragments: Vec<Option<Vec<u8>>>) -> anyhow::Result<Vec<u8>>;
    
    pub fn data_frags(&self) -> usize;   // 10 ou 4
    pub fn parity_frags(&self) -> usize; // 4 ou 2
    pub fn total_frags(&self) -> usize;  // 14 ou 6
}
```

> **Atenção:** RS(10,4) significa 10 dados + 4 paridade = 14 fragmentos totais. A reconstrução requer **quaisquer 10 dos 14** — NÃO "quaisquer 4". Tolera perda de **até 4** fragmentos.

### Topologia de Árvore — `prism-node/src/relay/tree.rs`

```rust
pub struct RelayNode {
    pub primary_parent:  Option<PeerId>,  // conexão QUIC ativa, recebe stream
    pub backup_parent:   Option<PeerId>,  // conexão QUIC aberta, stream pausada
    pub children:        Vec<PeerId>,     // máx 8 (fanout fixo)
}

impl RelayNode {
    /// Registra parent primário e abre conexão QUIC de backup simultaneamente.
    pub async fn set_parents(&mut self, primary: PeerId, backup: PeerId, swarm: &mut Swarm<_>);
    
    /// Propaga VideoChunk para todos os children via QUIC.
    /// Verifica streamer_sig antes de propagar — rejeita chunk inválido e penaliza upstream.
    pub async fn forward_chunk(&self, chunk: &VideoChunk, reputation: &PeerReputation) -> anyhow::Result<()>;
    
    /// Retorna true se este nó já tem 8 children (fanout esgotado).
    pub fn is_full(&self) -> bool { self.children.len() >= 8 }
}
```

### Failover — `prism-node/src/relay/failover.rs`

```rust
impl RelayNode {
    /// Chamado quando primary_parent desconecta (evento libp2p ConnectionClosed).
    /// Ativa backup_parent como novo primary em < 100ms.
    /// Inicia busca de novo backup via DHT assincronamente.
    pub async fn handle_parent_failure(&mut self, failed_peer: PeerId, swarm: &mut Swarm<_>);
}
```

**Protocolo de failover:**
```
1. Evento: ConnectionClosed(primary_parent)
2. Ativar backup_parent como primary (conexão QUIC já estava aberta — 0 handshake adicional)
3. Emitir métricas: "failover activated, downtime = Xms"
4. Assincronamente: FIND_NODE(stream_id + ":relay") na DHT para encontrar novo backup
5. Estabelecer nova conexão QUIC de backup em background
6. SLA: passos 1–2 em < 100ms; passo 4–5 em < 5s
```

### Sincronização — `prism-node/src/sync/playout.rs`

```rust
pub struct PlayoutBuffer {
    configured_latency_ms: u64,  // 7_000–20_000, configurável
}

impl PlayoutBuffer {
    /// Calcula deadline global para um chunk.
    /// deadline_global = chunk.timestamp_ms + configured_latency_ms
    pub fn deadline_for(&self, chunk: &VideoChunk) -> u64;
    
    /// Calcula tempo de espera para este nó.
    /// wait_ms = deadline_global - now_unix_ms()
    /// Se wait_ms <= 0: chunk já atrasado, entregar imediatamente ou descartar.
    pub fn wait_duration(&self, chunk: &VideoChunk) -> Option<tokio::time::Duration>;
}
```

**Sincronização de relógio:** usar NTP (chrony) como requisito de sistema do nó. Precisão de 10–50ms é suficiente para buffer de 7–20s. PTP disponível como opção avançada para operadores que precisam de < 1ms de drift.

### NACK — `prism-node/src/sync/nack.rs`

```rust
impl NackManager {
    /// Detecta gap na sequência recebida de chunks.
    /// Se gap detectado: envia NackRequest para upstream.
    pub async fn on_chunk_received(&mut self, chunk: &VideoChunk);
    
    /// Processa NackRequest recebido de downstream.
    /// Se chunk disponível em cache local: retransmite.
    /// Se não: propaga NackRequest upstream.
    /// Se deadline_ms já passou: descarta silenciosamente.
    pub async fn on_nack_received(&mut self, nack: NackRequest);
}
```

**Timeout de NACK:**
```
nack_timeout = nack.deadline_ms - now_unix_ms() - 200  // 200ms de margem de segurança
Se nack_timeout <= 0: descartar, usar frame concealment no viewer
Máx 5 NACKs pendentes por stream por peer — excesso recebe penalidade de reputação
```

### Classes de Nós

#### `class_a.rs` — Remux + Manifest
```rust
/// Recebe VideoChunk do seed (já verificado), remux para HLS fMP4,
/// gera manifests master.m3u8 e media.m3u8, assina com chave do nó,
/// propaga HlsManifest para nós Classe B.
pub async fn process_chunk_as_class_a(
    chunk:    &VideoChunk,
    identity: &Identity,
    // ...
) -> anyhow::Result<HlsManifest>;
```

**Verificação obrigatória:** verificar `streamer_sig` do VideoChunk antes de qualquer processamento. Chunk inválido → descartar + penalizar upstream.

#### `class_b.rs` — Transcode + RS Split
```rust
/// Transcodifica layer L0 do VideoChunk para H.264 Baseline (compatibilidade).
/// Gera thumbnail JPEG 320×180 a cada 30s de stream.
/// Fragmenta chunk via RS(10,4) e distribui fragmentos para nós Classe C.
pub async fn process_chunk_as_class_b(
    chunk: &VideoChunk,
    coder: &ReedSolomonCoder,
    // ...
) -> anyhow::Result<()>;
```

#### `class_c.rs` — Storage RS
```rust
/// Armazena fragmento RS recebido com TTL de 5 minutos.
/// Responde a pedidos de fragmento (para reconstrução).
/// Serve os últimos 200 chunks para late-joiners.
pub async fn store_fragment(frag: RsFragment) -> anyhow::Result<()>;
pub async fn serve_fragment(stream_id: &str, chunk_seq: u64, frag_index: u32) -> anyhow::Result<RsFragment>;
```

#### `edge.rs` — Layer Strip + HLS Delivery
```rust
/// Determina layers a servir baseado na bandwidth do viewer.
/// Faz strip das layers desnecessárias (operação bitwise no fMP4).
/// Serve segmentos HLS via HTTP (axum) ao viewer.
pub async fn serve_viewer(
    chunk:         &VideoChunk,
    viewer_bw_kbps: u32,
    // ...
) -> anyhow::Result<Vec<u8>>; // fMP4 com layers stripped
```

**Verificação de layer strip:** após strip, verificar que `SHA-256(stripped_payload)` corresponde ao `layer_hashes[layer_index].payload_hash` do VideoChunk original. Se não corresponder: chunk foi adulterado — descartar.

---

## Dependências Cargo

### `prism-encoder/Cargo.toml`
```toml
[package]
name = "prism-encoder"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core = { path = "../prism-core" }
anyhow     = "1"
tracing    = "0.1"

[build-dependencies]
bindgen    = "0.69"
```

**Sistema:** `libsvtav1` deve estar instalado no sistema (`apt install libsvtav1-dev` no Ubuntu).

### `prism-ingest/Cargo.toml`
```toml
[package]
name = "prism-ingest"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core    = { path = "../prism-core" }
prism-proto   = { path = "../prism-proto" }
prism-encoder = { path = "../prism-encoder" }
tokio         = { version = "1", features = ["full"] }
nokhwa        = { version = "0.11", features = ["input-native"] }  # captura câmera/tela
rml_rtmp      = "0.8"  # servidor RTMP para receber do OBS
anyhow        = "1"
tracing       = "0.1"
```

### Extensões de `prism-node/Cargo.toml`
```toml
# adicionar às dependências existentes da Fase 1:
reed-solomon-erasure = "6"
axum          = "0.7"
tower         = "0.4"
hyper         = { version = "1", features = ["full"] }
mp4           = "0.14"    # leitura/parse de fMP4 — NÃO usar para escrita ao vivo; segmentação fMP4 é responsabilidade do prism-encoder/src/segmenter.rs via bytes raw
bytes         = "1"
```

### `prism-studio/src-tauri/Cargo.toml`
```toml
[package]
name = "prism-studio"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core   = { path = "../../prism-core" }
prism-ingest = { path = "../../prism-ingest" }
tauri        = { version = "2", features = ["protocol-asset"] }
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
tokio        = { version = "1", features = ["full"] }
tracing      = "0.1"

[build-dependencies]
tauri-build = "2"
```

### `prism-player/package.json`
```json
{
  "name": "prism-player",
  "version": "0.1.0",
  "scripts": {
    "dev": "vite",
    "build": "vite build"
  },
  "dependencies": {
    "react": "^18",
    "react-dom": "^18",
    "hls.js": "^1.5"
  },
  "devDependencies": {
    "@types/react": "^18",
    "typescript": "^5",
    "vite": "^5"
  }
}
```

---

## Studio — Tauri 2 (básico, Fase 2)

### Comandos Tauri (`commands.rs`)

```rust
// Chamados pelo frontend React via invoke("start_stream", {...})

#[tauri::command]
async fn start_stream(
    window: tauri::Window,
    title: String,
    quality_preset: String,  // "low" | "medium" | "high" | "ultra"
) -> Result<String, String>;  // Ok(stream_id) | Err(mensagem)

#[tauri::command]
async fn stop_stream() -> Result<(), String>;

#[tauri::command]
async fn get_metrics() -> Result<StreamMetrics, String>;

#[derive(serde::Serialize)]
pub struct StreamMetrics {
    pub active_nodes: u32,
    pub viewer_count: u32,     // estimativa por pulls de manifest HLS
    pub bitrate_kbps: u32,
    pub latency_ms:   u32,     // latência configurada
    pub uptime_s:     u64,
}
```

**Eventos emitidos ao frontend (a cada 5s):**
```rust
window.emit("metrics-update", StreamMetrics { ... }).unwrap();
```

### Frontend Studio — componentes mínimos

```
Preview.tsx:
  - Exibe preview de câmera/tela a 15fps (para economizar CPU)
  - Mostra resolução e FPS atuais
  - Overlay de warning se CPU > 85%

NetworkHealth.tsx:
  - Escuta evento "metrics-update" via window.listen()
  - Exibe: nós ativos, viewers, bitrate, latência
  - Botões: [GO LIVE] e [ENCERRAR]

EncodeSettings.tsx:
  - Seletor de preset: Baixo / Médio / Alto / Ultra
  - Toggle de aceleração de hardware (VAAPI/NVENC/VideoToolbox)
```

---

## Player PWA — HLS.js (básico, Fase 2)

### `HlsPlayer.tsx`

```tsx
// Usa HLS.js para playback adaptativo
// ABR automático baseado em bandwidth estimado pelo HLS.js
// Exibe qualidade atual e opção de override manual

interface HlsPlayerProps {
  manifestUrl: string;   // URL do manifest HLS do edge node
  streamTitle: string;
}
```

**Requisitos mínimos:**
- Iniciar playback automaticamente quando `manifestUrl` é fornecido
- ABR automático sem intervenção do usuário
- Exibir qualidade atual (ex: "720p") com opção de override
- Fallback para H.264 se browser não suporta AV1 (usar `isSupported()` do HLS.js)

---

## Riscos e Pré-condições

### Risco 1 — Cold Start da Rede (Alto Impacto)

**Problema:** A Fase 2 assume que a DHT já tem nós seed operacionais para receber os chunks injetados pelo streamer. Sem pelo menos 2 seeds ativos, a rede simplesmente não existe — a implementação funcionará em testes locais mas falhará em produção desde o primeiro dia.

**Natureza do risco:** Produto e operações, não código.

**Mitigação obrigatória antes de considerar a Fase 2 concluída:**
- A equipe deve operar pelo menos 3 seed nodes geograficamente distintos antes do primeiro teste E2E com streamer real
- Os seeds devem estar listados como bootstrap peers embutidos no binário (conforme decidido na Fase 1)
- Critério C1 e C2 deste PRD só são válidos se testados com seeds externos, não localhost

**Ação:** Provisionar infraestrutura de seed nodes antes da Sprint 5.

---

### Risco 2 — SVT-AV1 Real-Time via FFI (Alto Impacto)

**Problema:** O encode AV1/SVC via SVT-AV1 é o gargalo técnico de maior incerteza desta fase. Se `prism-encoder` não atingir encode em tempo real no hardware alvo com presets 9–12, toda a pipeline de vídeo da Fase 2 trava — e não há fallback definido sem uma decisão explícita de arquitetura.

**Thresholds de viabilidade:**
- 1080p60 (bitrate total ~5.5 Mbps): encode deve completar em < 16ms por frame em hardware de streamer típico (CPU 6+ cores, 2020+)
- 720p30: encode em < 33ms por frame (mínimo aceitável)
- Se nenhum preset atingir tempo real: acionar fallback H.264 no streamer (documentado em `docs/architecture/overview.md §7`)

**Mitigação obrigatória:**
> **Executar spike isolado de `prism-encoder` ANTES de iniciar os demais módulos da Fase 2.**
>
> O spike deve: compilar o FFI SVT-AV1, encodar 60 frames YUV420 em loop, medir throughput. Se throughput < 1.0× tempo real em preset 12 no hardware de CI: parar e decidir fallback antes de continuar.

**Spike:** item 1 da Ordem de Implementação (Sprint 5–7) já reflete isso — `svt_av1.rs` é o primeiro arquivo a implementar. Não avançar para `segmenter.rs` sem validar throughput real.

---

## Ordem de Implementação

```
Sprint 5–7 (Semanas 9–14): Encode e Injeção
1. prism-encoder/src/svt_av1.rs         — FFI SVT-AV1 + teste de encode em tempo real
2. prism-encoder/src/svc_layers.rs
3. prism-encoder/src/segmenter.rs        — fMP4 de 3s
4. prism-ingest/src/rtmp_server.rs       — servidor RTMP local porta 1935
5. prism-ingest/src/capture.rs           — câmera/tela via nokhwa
6. prism-ingest/src/injector.rs          — sign + inject em 4 seeds
   Teste: chunk flui do streamer para 4 seeds com streamer_sig válida

Sprint 8–10 (Semanas 15–20): Topologia e Relay
7. prism-node/src/relay/tree.rs          — fanout=8, dual-parent QUIC
8. prism-node/src/relay/failover.rs      — ativação backup em < 100ms
9. prism-node/src/classes/class_a.rs     — remux HLS, manifest assinado
10. prism-node/src/classes/edge.rs       — layer strip, HTTP com axum
    Teste E2E: stream flui do host até viewer em player HLS padrão

Sprint 11–12 (Semanas 21–24): Confiabilidade
11. prism-node/src/erasure/reed_solomon.rs — RS(10,4) e RS(4,2)
12. prism-node/src/classes/class_b.rs    — transcode + RS split
13. prism-node/src/classes/class_c.rs    — storage RS
14. prism-node/src/sync/playout.rs       — playout buffer + deadline
15. prism-node/src/sync/nack.rs          — NACK seletivo
16. prism-studio/ (básico)               — Tauri 2 + Preview + NetworkHealth
17. prism-player/ (básico)               — PWA + HLS.js
    Teste de caos: 40% dos nós removidos — stream não interrompe
```

---

## Testes

### Unitários

```rust
// reed_solomon.rs

#[test]
fn rs_standard_reconstruct_from_any_10_of_14() {
    let coder = ReedSolomonCoder::new(RsConfig::Standard);
    let payload = vec![0xABu8; 9_000]; // 9KB representativo de chunk
    let frags = coder.encode(&payload).unwrap();
    assert_eq!(frags.len(), 14);
    
    // Testar 10 combinações distintas de 10 fragmentos (dos 14)
    for missing in 0..4 {
        let mut partial: Vec<Option<Vec<u8>>> = frags.iter().map(|f| Some(f.clone())).collect();
        partial[missing] = None;
        partial[missing + 1] = None;
        partial[missing + 2] = None;
        partial[missing + 3] = None;
        let recovered = coder.reconstruct(partial).unwrap();
        assert_eq!(recovered, payload);
    }
}

#[test]
fn rs_standard_fails_with_only_9_fragments() {
    let coder = ReedSolomonCoder::new(RsConfig::Standard);
    let payload = vec![0xCDu8; 9_000];
    let frags = coder.encode(&payload).unwrap();
    
    let mut partial: Vec<Option<Vec<u8>>> = frags.iter().map(|f| Some(f.clone())).collect();
    // Remover 5 fragmentos (precisa de 10, tem só 9)
    partial[0] = None; partial[1] = None; partial[2] = None;
    partial[3] = None; partial[4] = None;
    assert!(coder.reconstruct(partial).is_err());
}

// tree.rs

#[test]
fn relay_node_rejects_chunk_with_invalid_streamer_sig() {
    // Criar VideoChunk com streamer_sig adulterada
    // forward_chunk deve retornar Err e penalizar o upstream
}

// playout.rs

#[test]
fn playout_buffer_deadline_calculation() {
    let buffer = PlayoutBuffer { configured_latency_ms: 10_000 };
    let chunk = VideoChunk { timestamp_ms: 1_000_000, ..Default::default() };
    assert_eq!(buffer.deadline_for(&chunk), 1_010_000);
}
```

### Integração

**Teste E2E básico:**
```
1. Streamer inicia encode AV1/SVC em 720p60 (preset "high")
2. Injector assina e envia chunks para 4 seed nodes
3. Seeds → Classe A → edge node → viewer
4. PASSA:
   - Viewer recebe HLS válido em Chrome (verificado com HLS.js)
   - Latência streamer→viewer P95 < 20s (medido com timestamps)
   - Streamer_sig válida em 100% dos chunks no viewer
```

**Teste de failover:**
```
1. Stream ativa com 4 nós intermediários
2. Matar processo do nó primário de um nó filho (SIGKILL)
3. PASSA: stream não interrompe; failover em < 1s (medido com timestamps)
```

**Teste Reed-Solomon (caos):**
```
1. Stream com RS(10,4) ativo, 14 fragmentos distribuídos em 14 nós Classe C
2. Matar 4 nós Classe C simultaneamente
3. PASSA: chunk reconstruído com sucesso a partir dos 10 fragmentos restantes
4. Matar 5 nós: FAIL esperado — chunk irrecuperável, viewer usa frame concealment
```

**Teste de segurança — hijack de stream:**
```
1. Streamer legítimo transmite stream_id X
2. Nó malicioso anuncia stream_id X na DHT e envia chunks falsos
3. PASSA: receiver verifica streamer_sig de cada chunk contra streamer_pubkey do stream_record
         Chunks do nó malicioso são rejeitados (não têm streamer_privkey para assinar)
         Nó malicioso recebe penalidade de reputação e é banido
```

---

## Critérios de Conclusão

| # | Critério | Verificação |
|---|---------|-------------|
| C1 | Viewer recebe stream HLS válida em Chrome e Safari | Teste E2E |
| C2 | Latência streamer→viewer P95 < 20s em rede com 10+ nós | Medição com timestamps |
| C3 | Failover quando nó primário cai: < 1s de interrupção | Teste de failover |
| C4 | RS(10,4): chunk reconstruído com qualquer 10 dos 14 fragmentos | Teste RS unitário |
| C5 | RS(10,4): falha corretamente com apenas 9 fragmentos | Teste RS unitário |
| C6 | Chunk com `streamer_sig` inválida rejeitado em 100% dos casos | Teste de segurança |
| C7 | Tentativa de hijack rejeitada em < 100ms | Teste de segurança |
| C8 | Frame loss < 0,1% em rede estável (0% de perda de pacotes) | Medição por 30min |
| C9 | `prism-studio` inicia e exibe preview em < 2s | Teste manual |
| C10 | `prism-player` inicia playback HLS em < 5s após abrir URL | Teste manual |

---

## Fora do Escopo nesta Fase

- Chat e gossip → Fase 3
- Identidade do viewer no Player → Fase 3
- Stream discovery com UI polida → Fase 3
- State channels e pagamentos → Fase 4
- App mobile React Native → Fase 4
- OBS plugin nativo → Fase 4
- Testes de carga com 10.000+ viewers → Fase 4
