# Prism

Plataforma de live streaming descentralizada P2P. Sem servidores centrais — streamers e viewers formam a rede de distribuição.

## Como funciona

1. **Streamer** captura vídeo → encoder AV1/SVC gera até 4 camadas de qualidade → fragmenta com Reed-Solomon RS(10,4)
2. **Nós relay** armazenam e repassam fragmentos via DHT Kademlia → cada fragmento carrega assinatura Ed25519 do streamer
3. **Viewer** reconstrói o stream — quaisquer 10 dos 14 fragmentos RS são suficientes para recuperar o chunk original

Nenhum nó central conhece a identidade de streamer ou viewer. Um fragmento sem assinatura válida do streamer é descartado imediatamente.

## Stack

| Camada | Tecnologia |
|--------|-----------|
| P2P / DHT | rust-libp2p 0.54 (Kademlia + Gossipsub + QUIC) |
| Transporte | QUIC (quinn 0.11) — TCP + Noise XX como fallback |
| Criptografia | Ed25519 (ed25519-dalek 2) + SHA-256 |
| Codec | AV1/SVC via SVT-AV1 (FFI Rust com bindgen) |
| Redundância | Reed-Solomon RS(10,4) — reed-solomon-erasure |
| Desktop | Tauri 2 (Studio + Player) |
| Serialização | Protocol Buffers v3 (prost 0.13) |

## Fases de Desenvolvimento

| Fase | Crates | Status |
|------|--------|--------|
| 1 — Fundação P2P | `prism-core`, `prism-proto`, `prism-node` | Implementada |
| 2 — Pipeline de Vídeo | `prism-encoder`, `prism-ingest`, `prism-studio`, `prism-player` | Implementada |
| 3 — Chat & Identidade | `prism-chat` | Pendente |
| 4 — Produção & Resiliência | `prism-payments`, infra CI/CD, mobile | Pendente |

## Estrutura do Repositório

```
openstream/
├── crates/
│   ├── prism-core/       # Identidade Ed25519, reputação de peers
│   ├── prism-proto/      # Tipos Protobuf gerados (prost)
│   ├── prism-node/       # Nó P2P: DHT, relay, transferência de chunks (binário)
│   ├── prism-encoder/    # SVT-AV1 FFI, SVC layers, segmentador fMP4
│   ├── prism-ingest/     # RTMP server, captura A/V, assinatura e injeção
│   ├── prism-studio/     # App Tauri 2 para streamers
│   └── prism-player/     # App Tauri 2 para viewers
├── proto/                # Schemas .proto (Protobuf)
├── docs/
│   ├── architecture/     # Visão geral, stack, clientes
│   └── prd/              # PRDs por fase
└── CLAUDE.md             # Instruções para agentes de código
```

## Pré-requisitos

- **Rust** 1.78+ com `cargo`
- **libsvtav1** instalada no sistema (para encoding AV1)
  - Ubuntu/Debian: `sudo apt install libsvt-av1-dev`
  - macOS: `brew install svt-av1`
  - Windows: compilar do fonte ou usar variáveis `SVTAV1_INCLUDE_DIR` / `SVTAV1_LIB_DIR`
- **Node.js** 20+ e `npm` (para prism-studio e prism-player)
- **protoc** — não é necessário instalar; o build.rs de prism-proto usa protoc vendorizado

## Testes Manuais

### Suite completa

```bash
# Compilar e rodar todos os testes
cargo test --workspace

# Verificar warnings (obrigatório passar sem erros)
cargo clippy --workspace -- -D warnings
```

### prism-core — identidade e reputação

```bash
cargo test -p prism-core -- --nocapture
```

Cobre: geração de chaves, sign/verify, detecção de dados adulterados, persistência de chave em disco, penalização e ban de peers.

### prism-proto — tipos Protobuf

```bash
cargo build -p prism-proto
cargo test -p prism-proto
```

Valida que todos os `.proto` em `proto/` compilam e os tipos gerados funcionam.

### prism-node — nó P2P

```bash
cargo test -p prism-node
```

Para rodar o binário localmente:

```bash
# Criar config mínimo
cat > /tmp/node.toml <<EOF
key_path = "/tmp/prism_node.key"
listen_addr = "/ip4/127.0.0.1/tcp/4001"
bootstrap_peers = []
capacity_class = "C"
EOF

RUST_LOG=info cargo run -p prism-node -- --config /tmp/node.toml
```

O nó irá: gerar ou carregar chave Ed25519, medir capacidade (CPU + banda), publicar `NodeRecord` no DHT e aguardar conexões.

Para testar dois nós se conectando:

```bash
# Terminal 1 — nó bootstrap na porta 4001
RUST_LOG=info cargo run -p prism-node -- --config /tmp/node.toml

# Terminal 2 — segundo nó aponta para o primeiro (substituir <MULTIADDR> pelo endereço impresso pelo nó 1)
cat > /tmp/node2.toml <<EOF
key_path = "/tmp/prism_node2.key"
listen_addr = "/ip4/127.0.0.1/tcp/4002"
bootstrap_peers = ["<MULTIADDR>"]
capacity_class = "C"
EOF

RUST_LOG=info cargo run -p prism-node -- --config /tmp/node2.toml
```

### prism-encoder — AV1 e Reed-Solomon

```bash
cargo test -p prism-encoder -- --nocapture
```

Cobre: configuração das 4 camadas SVC (360p/480p/720p/1080p), segmentação fMP4, parsing de presets (`"low"/"medium"/"high"/"ultra"`). Se `libsvtav1` não estiver instalada, os testes de encode retornam `Err` e são sinalizados como skipped.

### prism-ingest — RTMP e injeção

```bash
cargo test -p prism-ingest -- --nocapture

# Opcional: com captura de câmera (requer nokhwa + hardware)
cargo test -p prism-ingest --features camera -- --nocapture
```

Para testar o servidor RTMP manualmente (requer `ffmpeg`):

```bash
# Terminal 1 — iniciar o servidor via teste ou binário integrado ao node
RUST_LOG=debug cargo test -p prism-ingest test_rtmp -- --nocapture

# Terminal 2 — enviar stream de teste com ffmpeg
ffmpeg -re -f lavfi -i testsrc=size=1280x720:rate=30 \
  -f lavfi -i sine=frequency=440 \
  -c:v libx264 -preset ultrafast -c:a aac \
  -f flv rtmp://127.0.0.1:1935/live/test-stream-key
```

### prism-studio — app desktop do streamer

```bash
cd crates/prism-studio
npm install
npm run dev        # modo desenvolvimento (Vite)
npm run tauri dev  # app Tauri completo
```

### prism-player — app desktop do viewer

```bash
cd crates/prism-player
npm install
npm run dev        # modo desenvolvimento (Vite)
npm run tauri dev  # app Tauri completo
```

## Flags de CLI — prism-node

| Flag | Padrão | Descrição |
|------|--------|-----------|
| `--config <PATH>` | `config.toml` | Caminho para o arquivo TOML de configuração |
| `--listen <ADDR>` | (do config) | Override do endereço de escuta (multiaddr) |
| `--capacity-class <CLASS>` | auto | Forçar classe `A`, `B`, `C` ou `edge` sem benchmark |

Variáveis de ambiente úteis:

| Variável | Exemplo | Efeito |
|----------|---------|--------|
| `RUST_LOG` | `info`, `debug`, `prism_node=trace` | Nível de log (tracing) |
| `SVTAV1_INCLUDE_DIR` | `/opt/svt-av1/include` | Headers customizados do SVT-AV1 |
| `SVTAV1_LIB_DIR` | `/opt/svt-av1/lib` | Libs customizadas do SVT-AV1 |

## Documentação

- [Arquitetura Geral](docs/architecture/overview.md)
- [Stack Tecnológico](docs/architecture/stack.md)
- [Clientes (Studio/Player)](docs/architecture/clients.md)
- **PRDs por fase:**
  - [Fase 1 — Fundação P2P](docs/prd/fase-1-fundacao-p2p.md)
  - [Fase 2 — Pipeline de Vídeo](docs/prd/fase-2-pipeline-video.md)
  - [Fase 3 — Chat & Identidade](docs/prd/fase-3-chat-identidade.md)
  - [Fase 4 — Produção & Resiliência](docs/prd/fase-4-producao-resiliencia.md)

## Segurança

Toda entrada externa (chunks, registros DHT, mensagens de chat) passa por verificação obrigatória nesta ordem: lista de banidos → parse Protobuf → `node_id = SHA-256(pubkey)` → hash do payload → assinatura Ed25519 → timestamp ±60s. Nenhuma etapa pode ser pulada.

Veja as [Regras de Segurança Não-Negociáveis](CLAUDE.md#regras-de-segurança-não-negociáveis) no CLAUDE.md.

## Licença

[A definir]
