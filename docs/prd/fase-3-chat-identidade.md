# PRD — Fase 3: Chat e Identidade

**Projeto:** Prism  
**Fase:** 3 de 4  
**Período:** Meses 8–10 | Sprints 13–15 | Semanas 25–30  
**Pré-requisito:** Fase 2 concluída (stream de vídeo funcionando E2E)  
**Desbloqueia:** Fase 4  

---

## Objetivo

Implementar o sistema de chat descentralizado em tempo real sobre a mesma rede P2P do vídeo, com ordenação causal de mensagens, histórico para late joiners, moderação distribuída e proteção contra spam. Integrar chat no Studio e Player. Implementar identidade do viewer no Player.

**O chat opera em wall clock real, completamente independente do buffer de vídeo.** Um viewer com 20s de buffer vê o chat do presente, não do passado.

---

## Modelo de Ameaças — Chat

| Ameaça | Descrição | Mitigação |
|--------|-----------|-----------|
| **Spam / flood de mensagens** | Usuário envia centenas de mensagens por segundo para saturar a rede gossip | Rate limiting por pubkey: máx 2 msg/s aceitas. Mensagens além do rate são descartadas localmente antes de propagar. Pubkey com spam recebe penalidade de reputação. |
| **Impersonação de usuário** | Nó malicioso envia mensagens assinadas com pubkey falsa | Toda mensagem carrega `sender_pubkey` + `signature`. Receptor verifica `Ed25519Sign` antes de exibir. Mensagem com assinatura inválida é descartada e propagação interrompida. |
| **Histórico forjado (anchor malicioso)** | Anchor node serve histórico de mensagens falso para late joiners | Cada mensagem armazenada mantém sua `signature` original. Viewer verifica assinatura de cada mensagem do histórico — mensagem com assinatura inválida é descartada individualmente (não invalida o histórico inteiro). |
| **Amplificação de gossip** | Mensagem maliciosa propagada em loop infinito | Deduplicação via LRU cache de SHA-256(message.id). Cada nó descarta mensagens já vistas. TTL máximo de 5 min por mensagem no cache. |
| **Mensagem com timestamp manipulado** | Atacante seta timestamp falso para inserir mensagem fora de ordem | Timestamp é informativo — não define ordem de exibição (vector clocks fazem isso). Timestamp muito no futuro (+5min) ou passado (-10min): mensagem exibida com aviso visual mas não descartada (usuários podem ter clocks desajustados). |
| **Blocklist forjada** | Nó malicioso propaga blocklist falsa atribuída ao streamer | Blocklist é assinada com a chave Ed25519 do streamer. Receptor verifica assinatura contra `streamer_pubkey` do `stream_record` na DHT. Blocklist sem assinatura válida é ignorada. |
| **Sybil no chat** | Atacante cria N identidades diferentes para contornar blocklist | Cada identidade custa geração de par Ed25519 (barato). Reputação por pubkey torna difícil abusar com identidades novas (sem histórico de reputação). Streamer pode bloquear por pubkey ou por range de comportamento. |
| **Anchor eclipse** | Atacante controla todos os anchors e serve histórico falso | Viewer consulta múltiplos anchors (mín 2) e verifica cada mensagem individualmente. Divergência entre anchors é loggada como aviso. |

---

## Estrutura de Arquivos

Adicionar ao workspace da Fase 2:

```
prism/
├── proto/
│   ├── chat_message.proto
│   ├── anchor_record.proto
│   └── blocklist.proto
└── crates/
    ├── prism-chat/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── gossip.rs           # configuração do libp2p gossipsub para chat
    │       ├── vector_clock.rs     # VectorClock: merge, happens-before, increment
    │       ├── ordering.rs         # buffer causal + prev-hash chain + entrega ordenada
    │       ├── anchor.rs           # eleição de anchor, storage de 500 msgs, API histórico
    │       ├── moderation.rs       # blocklist assinada + score de reputação por pubkey
    │       └── rate_limit.rs       # máx 2 msg/s por pubkey (sliding window)
    ├── prism-studio/               # extensão ao Studio da Fase 2
    │   └── src/
    │       └── components/
    │           ├── ChatPanel.tsx   # painel de chat integrado ao Studio
    │           └── ModerationMenu.tsx  # clique em usuário → bloquear/moderar
    └── prism-player/               # extensão ao Player da Fase 2
        └── src/
            ├── chat/
            │   ├── ChatView.tsx    # chat em tempo real com scroll
            │   └── ChatInput.tsx   # campo de mensagem (requer identidade)
            ├── identity/
            │   ├── IdentityModal.tsx   # criação de identidade Ed25519 via WebCrypto
            │   └── useIdentity.ts      # hook: gera chave, persiste em IndexedDB
            └── discovery/
                ├── StreamList.tsx      # grade de streams ativas
                └── useDhtDiscovery.ts  # consulta DHT por streams ativas
```

---

## Schemas Protobuf

### `proto/chat_message.proto`
```protobuf
syntax = "proto3";
package prism;

message ChatMessage {
  string id            = 1; // SHA-256(sender_pubkey || text || timestamp_ms) hex
  bytes  sender_pubkey = 2; // Ed25519 pubkey — 32 bytes
  string display_name  = 3; // nome escolhido pelo usuário — texto livre, max 32 chars UTF-8
  string stream_id     = 4; // stream a que esta mensagem pertence
  string text          = 5; // conteúdo — max 500 chars UTF-8
  uint64 timestamp_ms  = 6; // wall clock Unix ms — NÃO sincronizado com vídeo
  string prev_msg_id   = 7; // id da última mensagem que o sender viu (prev-hash chain)
  bytes  signature     = 8; // Ed25519Sign(privkey, SHA-256(campos 1-7 concatenados))
  map<string, uint64> vector_clock = 9; // pubkey_hex → contador causal
}
```

### `proto/anchor_record.proto`
```protobuf
syntax = "proto3";
package prism;

message AnchorRecord {
  bytes  anchor_node_id  = 1; // SHA-256(anchor_pubkey)
  bytes  anchor_pubkey   = 2; // Ed25519 pubkey do nó anchor
  string stream_id       = 3;
  uint32 stored_count    = 4; // número de mensagens armazenadas (máx 500)
  uint64 oldest_msg_ts   = 5; // timestamp da mensagem mais antiga armazenada
  uint64 expires_at_ms   = 6; // while stream active — renovado a cada 30s
  bytes  signature       = 7; // Ed25519Sign(anchor_privkey, hash(campos 1-6))
}
```

### `proto/blocklist.proto`
```protobuf
syntax = "proto3";
package prism;

message BlocklistEntry {
  bytes  blocked_pubkey = 1; // pubkey bloqueada
  string reason         = 2; // opcional — máx 100 chars
  uint64 blocked_at_ms  = 3; // quando o bloqueio foi aplicado
}

message Blocklist {
  string stream_id         = 1;
  bytes  streamer_pubkey   = 2;
  repeated BlocklistEntry entries = 3;
  uint64 version           = 4; // incrementado a cada atualização — peers usam versão mais alta
  bytes  signature         = 5; // Ed25519Sign(streamer_privkey, SHA-256(campos 1-4))
}
```

---

## Interfaces dos Módulos

### `prism-chat/src/vector_clock.rs`

```rust
use std::collections::HashMap;

/// Vetor de contadores causais: pubkey_hex → contador.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorClock(pub HashMap<String, u64>);

impl VectorClock {
    pub fn new() -> Self;
    
    /// Incrementa o contador para o sender.
    pub fn increment(&mut self, sender_pubkey_hex: &str);
    
    /// Merge: para cada entry, usa o máximo entre os dois clocks.
    pub fn merge(&mut self, other: &VectorClock);
    
    /// Retorna true se self causalmente precede other
    /// (self <= other em todas as entradas, com pelo menos uma estritamente menor).
    pub fn happens_before(&self, other: &VectorClock) -> bool;
    
    /// Retorna true se os clocks são concorrentes
    /// (nem A precede B, nem B precede A).
    pub fn concurrent_with(&self, other: &VectorClock) -> bool;
}
```

### `prism-chat/src/ordering.rs`

```rust
pub struct MessageOrderer {
    local_clock:    VectorClock,
    pending_buffer: BTreeMap<String, (ChatMessage, Instant)>, // msg_id → (msg, received_at)
    delivered:      LruCache<String, ()>,  // msg_ids já entregues (evita duplicatas)
}

impl MessageOrderer {
    pub fn new() -> Self;
    
    /// Processa mensagem recebida via gossip.
    /// Se a mensagem pode ser entregue (todos os prev já foram vistos): entrega imediatamente.
    /// Se prev desconhecido: coloca em pending_buffer com timestamp de recebimento.
    /// Mensagens no pending_buffer com mais de 30s são descartadas com marcador de gap.
    pub fn on_receive(&mut self, msg: ChatMessage) -> Vec<DeliveryEvent>;
    
    /// Verifica pending_buffer e entrega mensagens cujo prev agora é conhecido.
    /// Chame periodicamente (ex: a cada 100ms).
    pub fn flush_pending(&mut self) -> Vec<DeliveryEvent>;
}

pub enum DeliveryEvent {
    /// Mensagem entregue em ordem causal — exibir no chat.
    Message(ChatMessage),
    /// Gap irrecuperável (TTL expirado) — exibir indicador visual no chat.
    Gap { count: usize },
}
```

**Ordenação de mensagens concorrentes:**
- Mensagens com `concurrent_with` = true são ordenadas por `SHA-256(message.id)` lexicograficamente
- Determinístico: todos os nós chegam à mesma ordem sem comunicação adicional

### `prism-chat/src/gossip.rs`

```rust
pub struct ChatGossip {
    // Wraps libp2p gossipsub com configuração específica para chat
}

impl ChatGossip {
    /// Configura gossipsub com:
    ///   - mesh_n = 6 (conexões de overlay)
    ///   - mesh_n_low = 4
    ///   - mesh_n_high = 12
    ///   - gossip_lazy = 6
    ///   - fanout_ttl = 60s
    ///   - history_length = 5 heartbeats
    ///   - heartbeat_interval = 1s
    /// Topic: "prism/chat/{stream_id}"
    pub fn new(stream_id: &str) -> Self;
    
    /// Serializa, assina e publica ChatMessage.
    /// Verifica rate limit antes de publicar (máx 2/s por pubkey local).
    pub async fn publish(&self, msg: ChatMessage, identity: &Identity) -> anyhow::Result<()>;
    
    /// Verifica assinatura e deduplicação antes de aceitar mensagem recebida.
    /// Retorna None se assinatura inválida ou mensagem já vista.
    pub fn validate_incoming(&self, raw: &[u8]) -> Option<ChatMessage>;
}
```

### `prism-chat/src/anchor.rs`

```rust
pub struct AnchorNode {
    stream_id:     String,
    identity:      Arc<Identity>,
    message_store: Arc<Mutex<VecDeque<ChatMessage>>>,  // máx 500 mensagens
}

impl AnchorNode {
    pub fn new(stream_id: String, identity: Arc<Identity>) -> Self;
    
    /// Candidatar-se como anchor: publicar AnchorRecord na DHT.
    /// Chave DHT: sha256(b"prism:anchors:" || stream_id_bytes)
    /// Renovar a cada 30s enquanto stream ativa.
    pub async fn register(&self, kad: &mut Kademlia) -> anyhow::Result<()>;
    
    /// Adicionar mensagem ao histórico (buffer circular de 500).
    pub fn store_message(&self, msg: ChatMessage);
    
    /// Servir histórico para late joiner.
    /// Retorna até 500 mensagens mais recentes em ordem causal.
    /// Cada mensagem mantém sua assinatura original — receiver verifica.
    pub fn serve_history(&self, since_ts: Option<u64>) -> Vec<ChatMessage>;
}

/// Funcão usada pelo viewer ao entrar em uma stream.
/// Localiza anchors na DHT, consulta mínimo 2, verifica cada mensagem.
/// Mensagem com sig inválida: descartada individualmente, não falha a request inteira.
pub async fn fetch_history(
    stream_id: &str,
    kad:       &Kademlia,
    identity:  &Identity,   // para verificar assinaturas
) -> Vec<ChatMessage>;
```

### `prism-chat/src/moderation.rs`

```rust
pub struct ModerationEngine {
    active_blocklist: Option<Blocklist>,
    local_scores:     DashMap<String, i32>,  // pubkey_hex → score (-100 a 100)
}

impl ModerationEngine {
    pub fn new() -> Self;
    
    /// Atualizar blocklist a partir do gossip.
    /// Verifica assinatura do streamer antes de aplicar.
    /// Rejeita se Blocklist.version <= version atual (replay protection).
    pub fn apply_blocklist(&mut self, bl: Blocklist, streamer_pubkey: &VerifyingKey) -> anyhow::Result<()>;
    
    /// Retorna true se a mensagem deve ser exibida (não bloqueada, score >= threshold).
    pub fn should_display(&self, msg: &ChatMessage) -> bool;
    
    /// Reduz score por comportamento de spam.
    pub fn penalize(&self, pubkey_hex: &str, amount: i32);
    
    /// Retorna blocklist atual serializada + assinada (usado pelo streamer para publicar).
    pub fn export_blocklist(&self, stream_id: &str, identity: &Identity) -> Blocklist;
}
```

**Regras de score:**
| Evento | Efeito no score |
|--------|----------------|
| Mensagem válida, sem spam | +1 (máx 100) |
| Rate limit violado (>2/s) | -20 |
| Assinatura inválida | -50 |
| Adicionado à blocklist pelo streamer | Score → -100 (banido) |
| Score < -30 | Mensagens filtradas localmente |

### `prism-chat/src/rate_limit.rs`

```rust
pub struct ChatRateLimiter {
    // sliding window de 1s por pubkey
    counters: DashMap<String, (u32, Instant)>, // pubkey_hex → (count, window_start)
}

impl ChatRateLimiter {
    pub fn new() -> Self;
    
    /// Retorna true se a mensagem desta pubkey é permitida (≤ 2/s).
    /// Atualiza contador interno.
    pub fn allow(&self, pubkey_hex: &str) -> bool;
}
```

---

## Dependências Cargo

### `prism-chat/Cargo.toml`
```toml
[package]
name = "prism-chat"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core  = { path = "../prism-core" }
prism-proto = { path = "../prism-proto" }
libp2p      = { version = "0.54", features = ["gossipsub", "tokio", "macros"] }
tokio       = { version = "1", features = ["full"] }
dashmap     = "5"
lru         = "0.12"
prost       = "0.13"
anyhow      = "1"
thiserror   = "1"
tracing     = "0.1"
serde       = { version = "1", features = ["derive"] }
```

### Player — adições ao `package.json`
```json
{
  "dependencies": {
    "@noble/ed25519": "^2",
    "idb": "^8"
  }
}
```

**Nota:** `@noble/ed25519` implementa Ed25519 em TypeScript puro. Usar junto com `WebCrypto API` quando disponível (Chrome 113+, Firefox 119+, Safari 17+). Fallback para `@noble/ed25519` em browsers mais antigos.

---

## Identidade do Viewer (Player Web)

### `prism-player/src/identity/useIdentity.ts`

```typescript
interface Identity {
  publicKeyHex: string;    // 64 chars hex
  displayName: string;
  sign: (data: Uint8Array) => Promise<Uint8Array>;  // retorna assinatura Ed25519
}

// Hook React
export function useIdentity(): {
  identity: Identity | null;
  isLoading: boolean;
  create: (displayName: string, password?: string) => Promise<Identity>;
  load: () => Promise<Identity | null>;
  export: () => Promise<Blob>;  // arquivo JSON criptografado
};
```

**Fluxo de criação:**
```
1. Verificar suporte WebCrypto: window.crypto.subtle.generateKey("Ed25519", ...)
   Se não suportado: usar @noble/ed25519 como fallback
2. Gerar par Ed25519
3. Se password fornecida: criptografar chave privada com AES-GCM derivado via PBKDF2
4. Armazenar chave (possivelmente criptografada) no IndexedDB
5. Retornar Identity com pubkey e função sign()
```

**Armazenamento IndexedDB:**
```typescript
// Schema da store "prism-identity"
{
  publicKeyHex: string,
  privateKey: CryptoKey | Uint8Array,  // CryptoKey se WebCrypto; bytes se noble/fallback
  displayName: string,
  createdAt: number,
  encrypted: boolean,
}
```

**Aviso obrigatório na criação:**
> "Sua identidade é armazenada apenas neste browser. Se você limpar os dados do browser sem fazer backup, ela será perdida permanentemente."

---

## Descoberta de Streams (Player Web)

### `prism-player/src/discovery/useDhtDiscovery.ts`

```typescript
interface StreamInfo {
  streamId: string;
  streamerPubkeyHex: string;
  streamerDisplayName: string;
  title: string;
  viewerCount: number;   // estimativa
  thumbnailUrl: string;  // JPEG 320×180 do nó Classe A
  startedAt: number;     // Unix ms
}

export function useDhtDiscovery(): {
  streams: StreamInfo[];
  isLoading: boolean;
  search: (query: string) => void;  // busca por nome ou pubkey hex
};
```

**Implementação:**
- Consulta DHT via WebSocket para nó local (ou light client js-libp2p)
- Cache local em `sessionStorage`: streams descobertas persistem durante a sessão
- Streams privadas NÃO aparecem na lista (não anunciadas na DHT)
- Busca por pubkey hex: acesso direto via `FIND_VALUE(sha256("prism:stream:" || pubkey_hex))`

---

## Ordem de Implementação

```
Sprint 13 (Semanas 25–28): Gossip e Ordenação
1. prism-chat/src/vector_clock.rs
2. prism-chat/src/rate_limit.rs
3. prism-chat/src/gossip.rs        — gossipsub configurado para chat
4. prism-chat/src/ordering.rs      — buffer causal + prev-hash
   Teste: cobertura >99% em rede simulada de 100 nós em <500ms

Sprint 14 (Semanas 27–28): Anchors e Moderação
5. prism-chat/src/anchor.rs        — eleição, storage, histórico
6. prism-chat/src/moderation.rs    — blocklist assinada + scores
   Teste: late joiner recebe histórico em <2s
   Teste: blocklist propagada em <1s

Sprint 15 (Semanas 29–30): UI e Identidade
7. prism-player/src/identity/useIdentity.ts    — WebCrypto + IndexedDB
8. prism-player/src/identity/IdentityModal.tsx
9. prism-player/src/chat/ChatView.tsx
10. prism-player/src/chat/ChatInput.tsx
11. prism-player/src/discovery/             — StreamList + useDhtDiscovery
12. prism-studio/src/components/ChatPanel.tsx
13. prism-studio/src/components/ModerationMenu.tsx
    Teste E2E: chat funcional do Studio ao Player
```

---

## Testes

### Unitários

```rust
// vector_clock.rs

#[test]
fn happens_before_is_correctly_detected() {
    let mut vc_a = VectorClock::new();
    vc_a.increment("alice");
    
    let mut vc_b = vc_a.clone();
    vc_b.increment("bob");
    
    assert!(vc_a.happens_before(&vc_b));
    assert!(!vc_b.happens_before(&vc_a));
}

#[test]
fn concurrent_clocks_detected() {
    let mut vc_a = VectorClock::new();
    vc_a.increment("alice");
    
    let mut vc_b = VectorClock::new();
    vc_b.increment("bob");
    
    assert!(vc_a.concurrent_with(&vc_b));
    assert!(vc_b.concurrent_with(&vc_a));
}

#[test]
fn concurrent_messages_ordered_deterministically() {
    // Dois nós produzem a mesma ordenação para mensagens concorrentes
    let msg_a = make_chat_message("alice", "hello");
    let msg_b = make_chat_message("bob", "world");
    
    let order_1 = sort_concurrent(&[msg_a.clone(), msg_b.clone()]);
    let order_2 = sort_concurrent(&[msg_b.clone(), msg_a.clone()]);
    
    assert_eq!(order_1[0].id, order_2[0].id); // mesma ordem independente de input
}

// moderation.rs

#[test]
fn blocklist_with_invalid_signature_is_rejected() {
    let streamer = Identity::generate();
    let attacker = Identity::generate();
    
    let mut engine = ModerationEngine::new();
    
    // Blocklist assinada pelo atacante, mas atribuída ao streamer
    let fake_bl = make_blocklist_signed_by(&attacker, "some_stream");
    let result = engine.apply_blocklist(fake_bl, &streamer.verifying_key);
    assert!(result.is_err());
}

#[test]
fn older_blocklist_version_is_rejected() {
    let streamer = Identity::generate();
    let mut engine = ModerationEngine::new();
    
    let bl_v2 = make_blocklist_version(&streamer, 2);
    engine.apply_blocklist(bl_v2, &streamer.verifying_key).unwrap();
    
    let bl_v1 = make_blocklist_version(&streamer, 1); // versão antiga
    assert!(engine.apply_blocklist(bl_v1, &streamer.verifying_key).is_err());
}

// gossip.rs

#[test]
fn invalid_signature_rejected_before_propagation() {
    let gossip = ChatGossip::new("test-stream");
    let raw_msg = make_invalid_signature_raw_bytes();
    assert!(gossip.validate_incoming(&raw_msg).is_none());
}

#[test]
fn duplicate_message_rejected() {
    let gossip = ChatGossip::new("test-stream");
    let raw = make_valid_raw_message();
    
    let first = gossip.validate_incoming(&raw);
    assert!(first.is_some());
    
    let second = gossip.validate_incoming(&raw); // mesma mensagem
    assert!(second.is_none()); // deduplicada
}
```

### TypeScript

```typescript
// useIdentity.ts — testes com Vitest

test('create identity generates unique keys', async () => {
  const { create } = useIdentity();
  const id1 = await create('Alice');
  const id2 = await create('Bob');
  expect(id1.publicKeyHex).not.toBe(id2.publicKeyHex);
});

test('sign and verify roundtrip', async () => {
  const { create } = useIdentity();
  const id = await create('Alice');
  const data = new TextEncoder().encode('test message');
  const sig = await id.sign(data);
  // verificar com @noble/ed25519
  const pubkeyBytes = hexToBytes(id.publicKeyHex);
  const valid = await ed.verify(sig, data, pubkeyBytes);
  expect(valid).toBe(true);
});

test('tampered data fails verification', async () => {
  const { create } = useIdentity();
  const id = await create('Alice');
  const data = new TextEncoder().encode('original');
  const sig = await id.sign(data);
  const tampered = new TextEncoder().encode('tampered');
  const pubkeyBytes = hexToBytes(id.publicKeyHex);
  const valid = await ed.verify(sig, tampered, pubkeyBytes);
  expect(valid).toBe(false);
});
```

### Integração

**Teste E1 — Cobertura de gossip:**
```
1. Simular rede de 100 nós conectados via gossipsub
2. Nó 1 publica ChatMessage
3. Medir tempo até que todos os 99 outros nós recebam a mensagem
4. PASSA: cobertura 100% em < 500ms em P95
```

**Teste E2 — Ordenação causal:**
```
1. Alice envia "Pergunta" → Bob vê e responde "Resposta"
2. Com atraso de rede artificial, "Resposta" chega antes de "Pergunta" em alguns nós
3. PASSA: todos os nós exibem "Pergunta" antes de "Resposta" (ordering.rs buffer causal)
4. Zero casos de resposta antes de pergunta em 1000 execuções
```

**Teste E3 — Late joiner recebe histórico:**
```
1. Stream com 200 mensagens de chat já enviadas
2. Novo viewer se conecta
3. Viewer consulta anchors na DHT
4. PASSA: viewer recebe histórico em < 2s
5. PASSA: cada mensagem do histórico tem assinatura válida
```

**Teste E4 — Blocklist propagada:**
```
1. Streamer adiciona pubkey X à blocklist e assina
2. Blocklist propagada via gossip
3. PASSA: todos os viewers aplicam blocklist em < 1s
4. PASSA: mensagens de pubkey X não são exibidas em nenhum viewer
5. PASSA: blocklist forjada (assinatura inválida) ignorada por todos
```

**Teste E5 — Segurança: histórico de anchor malicioso:**
```
1. Anchor malicioso serve histórico com 1 mensagem falsificada (assinatura inválida)
2. Viewer consulta este anchor + 1 anchor honesto
3. PASSA: mensagem com assinatura inválida descartada individualmente
4. PASSA: as outras 499 mensagens legítimas são exibidas normalmente
5. NENHUM crash ou rejeição do histórico inteiro
```

---

## Critérios de Conclusão

| # | Critério | Verificação |
|---|---------|-------------|
| C1 | Gossip cobre 100% de 100 nós em < 500ms (P95) | Teste E1 |
| C2 | Ordenação causal: zero inversões em 1000 execuções | Teste E2 |
| C3 | Late joiner recebe 500 mensagens em < 2s | Teste E3 |
| C4 | Blocklist propagada em < 1s para todos os viewers | Teste E4 |
| C5 | Mensagem com assinatura inválida não exibida nem propagada | Testes unitários |
| C6 | Blocklist com assinatura inválida ignorada | Teste E4 + unitário |
| C7 | Rate limit: > 2 msg/s por pubkey descartado localmente | Teste unitário |
| C8 | Identidade criada no Player via WebCrypto sem contato externo | Inspecionar tráfego de rede |
| C9 | Descoberta de streams exibe lista sem servidor central | Teste manual |
| C10 | Chat E2E funcional: Studio publica → Player recebe em < 500ms | Teste manual |

---

## Fora do Escopo nesta Fase

- State channels e pagamentos → Fase 4
- App mobile React Native → Fase 4
- OBS plugin nativo → Fase 4
- Testes de carga com 10.000 viewers → Fase 4
- PWA install flow e notificações push → Fase 4
