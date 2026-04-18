# Prism — Instruções para o Agente

## O que é este projeto

**Prism** é uma plataforma de streaming de vídeo ao vivo 100% descentralizada e P2P. Nenhum servidor central existe — toda coordenação é feita via DHT Kademlia. Streams são verificadas criptograficamente: cada chunk de vídeo é assinado com Ed25519 pelo streamer, e qualquer nó que serve dados inválidos é detectado e penalizado.

---

## Mapa de Documentação

Antes de implementar qualquer feature, leia o documento relevante.

```
docs/
├── architecture/
│   ├── overview.md        # Arquitetura completa, requisitos funcionais e não-funcionais
│   ├── stack.md           # Justificativa de cada tecnologia e alternativas descartadas
│   └── clients.md         # Especificação de UI/UX: Studio (streamer) e Player (viewer)
└── prd/
    ├── fase-1-fundacao-p2p.md       # Fase atual se o workspace ainda não existe
    ├── fase-2-pipeline-video.md
    ├── fase-3-chat-identidade.md
    └── fase-4-producao-resiliencia.md
```

**Para determinar a fase atual:** verificar se `crates/` existe na raiz. Se não existir → Fase 1. Se existir, verificar qual PRD tem entregáveis não concluídos baseado nos testes de aceitação descritos em cada PRD.

---

## Stack Tecnológico

| Componente | Tecnologia | Versão |
|-----------|-----------|--------|
| Linguagem core | Rust + Tokio async | edition 2021, tokio 1 |
| P2P / DHT | rust-libp2p | 0.54 |
| Transporte | QUIC via quinn | 0.11 |
| Criptografia | ed25519-dalek | 2 |
| Encoder AV1 | SVT-AV1 via FFI (bindgen) | libsvtav1 instalado no sistema |
| Reed-Solomon | reed-solomon-erasure | 6 |
| Serialização | Protocol Buffers v3 + prost | prost 0.13 |
| Studio (desktop) | Tauri 2 + React + TypeScript | tauri 2, React 18 |
| Player (web) | React + TypeScript + HLS.js | HLS.js 1.5, Vite 5 |
| Player (mobile) | React Native + Expo | Expo 51 |
| Identidade browser | WebCrypto API + @noble/ed25519 | @noble/ed25519 ^2 |
| HTTP (edge node) | axum | 0.7 |
| Concorrência maps | dashmap | 5 |
| LRU cache | lru | 0.12 |

**Nunca substituir tecnologias sem consultar `docs/architecture/stack.md`.** Cada decisão foi justificada contra alternativas específicas.

---

## Estrutura do Workspace Cargo

```
prism/                          ← raiz do repositório (= openstream/)
├── Cargo.toml                  # workspace root — members listados aqui
├── CLAUDE.md                   # este arquivo
├── proto/                      # .proto compartilhados — todos compilados pelo prism-proto
│   ├── node_record.proto
│   ├── chunk_transfer.proto
│   ├── video_chunk.proto
│   ├── hls_manifest.proto
│   ├── rs_fragment.proto
│   ├── nack_request.proto
│   ├── chat_message.proto
│   ├── anchor_record.proto
│   └── blocklist.proto
├── docs/                       # documentação — não alterar sem instrução explícita
└── crates/
    ├── prism-core/             # identidade Ed25519, hash SHA-256, reputação de peers
    ├── prism-proto/            # código gerado pelo prost (build.rs compila os .proto)
    ├── prism-node/             # nó da rede: DHT, QUIC, classes A/B/C/edge, relay, RS, sync
    ├── prism-encoder/          # SVT-AV1 FFI + segmentador fMP4
    ├── prism-ingest/           # captura A/V, RTMP server, sign + inject em seeds
    ├── prism-chat/             # gossipsub, vector clocks, anchor nodes, moderação
    ├── prism-payments/         # state channels off-chain
    ├── prism-telemetry/        # coleta e agregação distribuída de métricas
    ├── prism-studio/           # app desktop do streamer (Tauri 2 + React)
    └── prism-player/           # PWA viewer (React) + mobile (React Native / Expo)
```

Cada crate tem seu próprio `Cargo.toml`. O workspace `Cargo.toml` na raiz lista todos os members.

---

## Regras de Segurança (Não-Negociáveis)

Estas regras se aplicam a **todo** código escrito neste projeto, sem exceção.

### 1. Chave Privada Ed25519

- A chave privada **NUNCA** aparece em logs, mensagens de erro, payloads de rede ou variáveis de ambiente
- Usar `zeroize` para limpar bytes da chave privada da memória quando não em uso
- Em Rust: o campo `signing_key: SigningKey` é sempre privado (sem `pub`)
- Em mobile: delegar operações de assinatura ao Secure Enclave (iOS) / Android Keystore — nunca extrair os bytes

### 2. Verificação Obrigatória Antes de Processar Qualquer Dado Externo

Para **todo** dado recebido de outro peer, executar nesta ordem antes de qualquer processamento:

```
Para ChunkTransfer / VideoChunk:
  1. SHA-256(sender_pubkey) == sender_node_id
  2. SHA-256(payload) == payload_hash
  3. Ed25519 signature é válida
  4. |timestamp_ms - now| <= 60_000 ms

Para NodeRecord:
  1. SHA-256(pubkey) == node_id
  2. expires_at_ms > now (não expirado)
  3. expires_at_ms <= now + 15_000 (TTL não manipulado)
  4. Ed25519 signature é válida
  5. capacity_class está em {"A", "B", "C", "edge"}

Para Blocklist:
  1. Ed25519 signature do streamer é válida
  2. version > versão atual (sem replay)

Para ChatMessage:
  1. Ed25519 signature é válida
  2. SHA-256(sender_pubkey || text || timestamp_ms) == id
```

**Se qualquer verificação falhar:** chamar `reputation.penalize(node_id, penalty, reason)` e retornar `Err`. Nunca ignorar silenciosamente.

### 3. Penalidades de Reputação

| Evento | Penalidade |
|--------|-----------|
| node_id ≠ SHA-256(pubkey) | -30 |
| Payload hash inválido | -25 |
| Assinatura Ed25519 inválida | -25 |
| Timestamp fora do range | -15 |
| NodeRecord expirado ou TTL longo demais | -20 |
| Protobuf malformado | -10 |

Score < -50 → ban de 60 segundos. Peer banido: rejeitar conexão imediatamente.

### 4. RS(10,4) — Descrição Correta

RS(10,4) = **10 fragmentos de dados + 4 de paridade = 14 total**.  
Reconstituição requer **quaisquer 10 dos 14**.  
Tolera perda de **até 4** fragmentos.  
**Nunca escrever "qualquer 4 fragmentos reconstroem" — isso está errado.**

### 5. Geração de Chave

- Usar `rand::rngs::OsRng` — **nunca** `thread_rng()`, `SmallRng` ou seeds fixas
- Arquivo de chave privada: permissões `0600` (Unix) ou ACL restritiva (Windows)
- `load()` emite `tracing::warn!` se permissões estiverem abertas (world-readable)

---

## Padrões de Código Rust

### Tratamento de Erros

```rust
// Em library crates (prism-core, prism-chat, etc.): usar thiserror
#[derive(thiserror::Error, Debug)]
pub enum MyError { ... }

// Em binary crates (prism-node/main.rs): usar anyhow
fn main() -> anyhow::Result<()> { ... }

// NUNCA usar unwrap() em código de produção — apenas em testes
// NUNCA usar expect() com mensagem vaga — se usar, a mensagem deve explicar o invariante

// CORRETO:
let value = map.get(&key).ok_or_else(|| anyhow::anyhow!("key not found: {key}"))?;

// ERRADO:
let value = map.get(&key).unwrap();
```

### Logging

```rust
// Usar tracing — nunca println! em código de produção
tracing::info!("chunk received: seq={} stream={}", chunk.sequence, &chunk.stream_id[..8]);
tracing::warn!("peer penalized: node_id={} reason={}", hex::encode(&node_id[..4]), reason);
tracing::error!("failed to publish NodeRecord: {err}");

// NUNCA logar chaves privadas ou payloads completos de vídeo
// Truncar identificadores nos logs: &node_id[..4] (primeiros 4 bytes = 8 chars hex)
```

### Concorrência

```rust
// Para estado compartilhado entre tasks: Arc<Mutex<T>> ou Arc<RwLock<T>>
// Para mapas concorrentes sem lock global: DashMap<K, V>
// Channels de comunicação entre tasks: tokio::sync::mpsc ou broadcast

// NUNCA usar std::sync::Mutex em código async — usar tokio::sync::Mutex
// NUNCA bloquear em código async (.await em operações bloqueantes → tokio::task::spawn_blocking)
```

### Testes

```rust
// Testes unitários: no mesmo arquivo, módulo #[cfg(test)]
// Testes de integração: em tests/ na raiz da crate
// Usar tempfile::NamedTempFile para arquivos temporários — nunca criar em /tmp manualmente

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]  // para funções async
    async fn test_nome_descritivo() { ... }
    
    #[test]         // para funções síncronas
    fn test_nome_descritivo() { ... }
}
```

---

## Padrões de Código TypeScript / React

```typescript
// Sem any — usar tipos explícitos ou unknown
// Async/await em vez de .then()
// Componentes funcionais com hooks — sem class components
// Props com interface, nunca type inline anônimo
// Erros: try/catch com mensagem útil — nunca catch silencioso

// CORRETO:
async function fetchStream(streamId: string): Promise<StreamInfo> {
  try {
    const result = await dht.findValue(streamId);
    return parseStreamInfo(result);
  } catch (err) {
    console.error(`Failed to fetch stream ${streamId}:`, err);
    throw err;
  }
}
```

---

## Fluxo de Trabalho do Agente

### Ao Iniciar Uma Sessão

1. Ler este arquivo (`CLAUDE.md`)
2. Verificar em qual fase o projeto está (existência de `crates/`, quais testes passam)
3. Ler o PRD da fase atual em `docs/prd/`
4. Implementar os entregáveis do PRD na **ordem de implementação** definida nele
5. Após cada módulo: escrever testes, executar `cargo test -p <crate>` (ou equivalente)

### Antes de Criar Qualquer Arquivo

Verificar se o arquivo/módulo já existe. Se existir, ler antes de editar.

### Ao Implementar um Módulo

1. Ler a seção correspondente no PRD da fase
2. Implementar a interface exata definida (assinaturas de função, tipos, enums)
3. Aplicar todas as regras de segurança relevantes
4. Escrever os testes descritos no PRD
5. Executar: `cargo clippy -p <crate> -- -D warnings`
6. Executar: `cargo test -p <crate>`
7. Só avançar para o próximo módulo se os testes passarem

### Comandos de Verificação

```bash
# Verificar toda a workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings

# Verificar crate específica
cargo test -p prism-core
cargo clippy -p prism-node -- -D warnings

# Compilar proto (executar no prism-proto)
cargo build -p prism-proto

# Frontend do Studio
cd crates/prism-studio && npm run dev

# Player web
cd crates/prism-player && npm run dev
```

### Ao Encontrar Inconsistência entre Código e Documentação

- A documentação em `docs/` é a fonte de verdade para decisões de arquitetura e interfaces
- Se a documentação tiver erro técnico óbvio: corrigi-lo e continuar
- Se for ambiguidade de design: parar e pedir esclarecimento ao usuário

---

## O Que Nunca Fazer

- `unwrap()` em código de produção (apenas em testes)
- `println!` em código de produção (usar `tracing::info!`)
- Aceitar dados de rede sem verificar assinatura Ed25519
- Logar chave privada, bytes de chave ou payload completo de chunk
- Usar `rand::thread_rng()` para geração de chaves criptográficas
- Usar Go, Python, Node.js no backend — o core é exclusivamente Rust
- Ignorar erro de `cargo clippy` (`-D warnings` é obrigatório)
- Criar arquivo de configuração novo sem verificar se já existe no PRD
- Substituir tecnologia do stack sem consultar `docs/architecture/stack.md`
