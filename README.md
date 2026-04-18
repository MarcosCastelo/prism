# Prism

Plataforma de live streaming descentralizada P2P. Sem servidores centrais — streamers e viewers formam a rede de distribuição.

## Como funciona

1. **Streamer** captura vídeo → encoder AV1/SVC gera 4 camadas de qualidade → fragmenta com Reed-Solomon RS(10,4)
2. **Nós relay** armazenam e repassam fragmentos via DHT Kademlia → cada fragmento carrega assinatura Ed25519 do streamer
3. **Viewer** reconstrói o stream — qualquer 10 dos 14 fragmentos RS são suficientes para recuperar o chunk original

Nenhum nó central conhece a identidade de streamer ou viewer. Um fragmento sem assinatura válida do streamer é descartado imediatamente.

## Stack

| Camada | Tecnologia |
|--------|-----------|
| P2P / DHT | rust-libp2p 0.54 (Kademlia + Gossipsub + QUIC) |
| Transporte | QUIC (quinn 0.11) — WebRTC só como fallback CGNAT |
| Criptografia | Ed25519 (ed25519-dalek 2) + SHA-256 |
| Codec | AV1/SVC via SVT-AV1 (FFI Rust com bindgen) |
| Redundância | Reed-Solomon RS(10,4) — reed-solomon-erasure |
| Desktop | Tauri 2 (Studio + Player) |
| Mobile | React Native + Expo 51 (Player) |
| Sincronização | NTP (chrony); PTP opcional para operadores |

## Fases de Desenvolvimento

| Fase | Crates | Status |
|------|--------|--------|
| 1 — Fundação P2P | `prism-core`, `prism-proto`, `prism-node` | Em desenvolvimento |
| 2 — Pipeline de Vídeo | `prism-encoder`, `prism-relay`, `prism-hls-edge` | Pendente |
| 3 — Chat & Identidade | `prism-chat`, `prism-studio` (UI), `prism-player` (UI) | Pendente |
| 4 — Produção & Resiliência | `prism-payments`, infra CI/CD, mobile | Pendente |

## Estrutura do Repositório

```
openstream/
├── crates/                  # Workspace Cargo
│   ├── prism-core/          # Identidade Ed25519, reputação de peers
│   ├── prism-proto/         # Tipos Protobuf gerados
│   ├── prism-node/          # Nó P2P: DHT, relay, transferência de chunks
│   ├── prism-encoder/       # SVT-AV1, SVC, Reed-Solomon (Fase 2)
│   ├── prism-relay/         # Árvore de distribuição, failover (Fase 2)
│   ├── prism-hls-edge/      # Servidor HLS edge (Fase 2)
│   ├── prism-chat/          # Mensagens gossip com ordenação causal (Fase 3)
│   ├── prism-studio/        # App Tauri 2 para streamers (Fase 3)
│   ├── prism-player/        # App Tauri 2 / React Native para viewers (Fase 3)
│   └── prism-payments/      # State channels off-chain (Fase 4)
├── proto/                   # Schemas .proto (Protobuf)
├── docs/
│   ├── architecture/        # Visão geral, stack, clientes
│   └── prd/                 # PRDs por fase (referência para implementação)
└── CLAUDE.md                # Instruções para agentes de código
```

## Documentação

- [Arquitetura Geral](docs/architecture/overview.md)
- [Stack Tecnológico](docs/architecture/stack.md)
- [Clientes (Studio/Player)](docs/architecture/clients.md)
- **PRDs por fase:**
  - [Fase 1 — Fundação P2P](docs/prd/fase-1-fundacao-p2p.md)
  - [Fase 2 — Pipeline de Vídeo](docs/prd/fase-2-pipeline-video.md)
  - [Fase 3 — Chat & Identidade](docs/prd/fase-3-chat-identidade.md)
  - [Fase 4 — Produção & Resiliência](docs/prd/fase-4-producao-resiliencia.md)

## Desenvolvimento com Agentes de IA

Este projeto foi estruturado para ser implementado por agentes de código (Claude Code). O `CLAUDE.md` na raiz define regras de segurança, padrões de código e fluxo de trabalho. Cada PRD contém interfaces Rust exatas, casos de teste e critérios de aceitação verificáveis.

Veja [docs/guia-agente.md](docs/guia-agente.md) para instruções completas de como usar os agentes e skills disponíveis.

## Início Rápido (Desenvolvimento)

```bash
# Verificar o ambiente
cargo check --workspace

# Identificar a fase atual e próximo passo
# (dentro do Claude Code)
/fase-atual

# Executar CI local antes de avançar de fase
/ci
```

## Segurança

Toda entrada externa (chunks, registros DHT, mensagens de chat) passa por verificação obrigatória na seguinte ordem: lista de banidos → parse Protobuf → node_id = SHA-256(pubkey) → hash do payload → assinatura Ed25519 → timestamp ±60s. Nenhuma dessas etapas pode ser pulada.

Veja as [Regras de Segurança Não-Negociáveis](CLAUDE.md#regras-de-segurança-não-negociáveis) no CLAUDE.md.

## Licença

[A definir]
