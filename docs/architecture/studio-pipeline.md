# Studio → Ingest → Node: Pipeline de Integração

## Contexto

O PRD da Fase 2 especifica os módulos do pipeline de vídeo de forma isolada
(encoder, ingest, relay, edge). Este documento descreve como eles são
orquestrados pelo Studio (aplicação Tauri 2 do streamer) para produzir um
fluxo E2E funcional.

---

## Diagrama de Fluxo

```
┌─────────────────────────────────────────────────────────────────┐
│  prism-studio (Tauri 2)                                         │
│                                                                 │
│  GO LIVE → start_stream()                                       │
│               │                                                 │
│               ├── load Identity (prism_studio.key)              │
│               ├── EmbeddedSeedRouter (PRISM_SEED_ADDRS env)     │
│               ├── StreamInjector::new(identity, ts, layers, router)│
│               └── spawn run_ingest_pipeline()                   │
│                          │                                      │
│                          ├── RtmpServer::new(1935).run()        │
│                          │       ↓ MediaFrame (FLV video)       │
│                          ├── acumula frames em janelas de 3s    │
│                          │       ↓ Segment { data, duration_ms }│
│                          └── StreamInjector::inject(segment)    │
│                                  │                              │
└──────────────────────────────────┼──────────────────────────────┘
                                   │  TCP (host:port)
                                   ↓
┌──────────────────────────────────────────────────────────────────┐
│  prism-node (seed A, B, C…)                                      │
│                                                                  │
│  recebe VideoChunk (protobuf, length-prefix framing)             │
│     → verifica Ed25519 streamer_sig                              │
│     → class_a: gera HlsManifest → edge.store_manifest()         │
│     → edge: serve HTTP                                           │
│         GET /{stream_id}/master.m3u8                             │
│         GET /{stream_id}/{layer}/media.m3u8                      │
│         GET /{stream_id}/{seq}.m4s                               │
└──────────────────────────────────────────────────────────────────┘
                                   ↓
┌──────────────────────────────────────────────────────────────────┐
│  prism-player (HLS.js)                                           │
│                                                                  │
│  URL: http://<edge-ip>:8080/<stream_id>/master.m3u8              │
└──────────────────────────────────────────────────────────────────┘
```

---

## Decisão de Arquitetura: Opção A — Embedded Router

O Studio **não** cria um nó P2P completo. Ele usa um `EmbeddedSeedRouter`
leve que implementa o trait `SeedRouter` de `prism-ingest` via TCP simples
(protocolo length-prefix). Isso mantém o Studio desacoplado de `libp2p` e
concentra toda a lógica de rede nos nós `prism-node`.

### Por que não Opção B (Studio instancia libp2p direto)?

- Duplicaria lógica de identidade, DHT e QUIC já presente em `prism-node`
- Aumentaria o tamanho binário e o tempo de startup do app Tauri
- Conflito de porta: Studio e Node competiriam por portas P2P

---

## Componentes Envolvidos

| Crate | Papel |
|-------|-------|
| `prism-studio/src-tauri` | Orquestra o pipeline; expõe comandos Tauri |
| `prism-core` | Identidade Ed25519 do streamer |
| `prism-ingest` | `RtmpServer` + `StreamInjector` + trait `SeedRouter` |
| `prism-encoder` | Tipo `Segment` (payload do chunk) |
| `prism-node` | `EmbeddedSeedRouter` (TCP); lógica de relay e HLS |

---

## Protocolo de Entrega de Chunk (TCP)

O `EmbeddedSeedRouter` usa framing simples sobre TCP:

```
┌──────────────┬──────────────────────────────────────┐
│ length: u32BE │ VideoChunk serializado (protobuf)    │
└──────────────┴──────────────────────────────────────┘
```

Cada seed recebe o chunk em paralelo (tokio::spawn por seed). Timeout: 2s por
seed. A injeção falha se menos de 2 seeds aceitarem o chunk.

---

## Configuração

### Variável de ambiente

```bash
# Endereços dos nós seed no formato host:port (porta TCP de recepção de chunks)
PRISM_SEED_ADDRS=192.168.1.10:4002,192.168.1.11:4002
```

### Identidade do streamer

Gerada automaticamente em `prism_studio.key` no diretório de trabalho na
primeira execução. Reutilizada nas execuções seguintes — o `stream_id` é
derivado dessa chave + timestamp de início.

---

## Como Iniciar uma Stream (Manual)

### 1. Iniciar seed nodes

```bash
# Em cada máquina seed:
cat > /tmp/seed.toml <<EOF
key_path = "/tmp/seed.key"
listen_addr = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []
capacity_class = "A"
EOF

RUST_LOG=info cargo run -p prism-node -- --config /tmp/seed.toml
```

### 2. Configurar e iniciar o Studio

```bash
cd crates/prism-studio

# Apontar para os seed nodes (porta 4002 = porta de recepção de chunks)
export PRISM_SEED_ADDRS="<ip-seed-1>:4002,<ip-seed-2>:4002"

npx tauri dev
```

### 3. Configurar OBS

```
Servidor RTMP: rtmp://localhost/live
Chave de stream: qualquer string
```

Clicar **GO LIVE** no Studio → OBS conecta na porta 1935 → frames fluem
para os seeds.

### 4. Assistir no Player

```bash
cd crates/prism-player
npm run dev

# No player, inserir a URL do manifest:
http://<ip-seed>:8080/<stream_id>/master.m3u8
```

O `stream_id` é exibido no Studio após clicar GO LIVE.

---

## Estado Atual vs. Planejado

| Etapa | Estado |
|-------|--------|
| Identity Ed25519 | ✅ Implementado |
| RTMP server (porta 1935) | ✅ Implementado |
| Agrupamento em segmentos de 3s | ✅ Implementado (payload FLV raw) |
| Assinatura Ed25519 do chunk | ✅ Implementado (`StreamInjector`) |
| Entrega TCP aos seeds | ✅ Implementado (`EmbeddedSeedRouter`) |
| Encode AV1/SVC antes de injetar | 🔲 Futuro (requer `libsvtav1` no Studio) |
| Descoberta de seeds via DHT | 🔲 Futuro (`EmbeddedSeedRouter::find_seeds` usa config estática) |
| Recepção TCP no seed node | ✅ Implementado (`transfer::chunk_receiver`, porta 4002) |
| Edge HTTP server (porta 8080) | ✅ Implementado (`classes::edge::run_edge_server`) |
