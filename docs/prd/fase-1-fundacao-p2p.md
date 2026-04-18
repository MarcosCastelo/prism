# PRD — Fase 1: Fundação P2P

**Projeto:** Prism  
**Fase:** 1 de 4  
**Período:** Meses 1–3 | Sprints 1–4 | Semanas 1–8  
**Pré-requisito:** Nenhum  
**Desbloqueia:** Fases 2, 3 e 4  

---

## Objetivo

Implementar a camada P2P base do Prism em Rust. Ao final desta fase: nós se descobrem via DHT Kademlia sem servidor central, conectam-se via QUIC com autenticação Ed25519, atravessam NAT automaticamente, anunciam capacidade na DHT, rejeitam dados corrompidos ou de origem duvidosa, e penalizam peers que enviam dados inválidos.

**Nenhum vídeo, chat ou UI é implementado nesta fase.**

---

## Modelo de Ameaças

Esta seção define as ameaças que a Fase 1 deve mitigar. O agente deve implementar cada mitigação explicitamente — não como melhoria futura.

| Ameaça | Descrição | Mitigação implementada nesta fase |
|--------|-----------|----------------------------------|
| **Impersonação de streamer** | Nó malicioso anuncia stream_id como se fosse o streamer legítimo | `stream_id = SHA-256(streamer_pubkey \|\| start_ts)` — vinculado à chave. Todo chunk carrega `streamer_sig` verificável. Sem a chave privada, não é possível assinar chunks válidos. |
| **Chunk infectado / adulterado** | Relay modifica payload de um chunk em trânsito | `payload_hash` + assinatura Ed25519 do remetente. Qualquer alteração de 1 bit é detectada. Peer que envia chunk inválido recebe penalidade. |
| **NodeRecord forjado** | Nó malicioso publica NodeRecord com node_id de outro nó | NodeRecord é assinado pelo próprio nó com sua chave Ed25519. Receptor verifica assinatura antes de aceitar qualquer registro na routing table. |
| **Ataque Sybil na DHT** | Criar muitos node_ids para controlar o roteamento DHT | Limite de conexões por IP (máx 5 peers distintos no mesmo /24). Diversidade obrigatória de K-buckets. Sem taxa de registro: qualquer nó pode entrar, mas peers com reputação baixa são evitados. |
| **Ataque Eclipse** | Isolar um nó cercando-o com peers maliciosos | Manter conexões em pelo menos 3 K-buckets distintos do XOR space. Se < 3 buckets cobertos, buscar peers adicionais ativamente. |
| **Replay Attack** | Reenviar chunk/registro antigo com assinatura válida | `timestamp_ms` validado: rejeitar se `|now - timestamp| > 60s`. `expires_at_ms` em NodeRecord: rejeitar se expirado. |
| **DoS por NACKs / conexões** | Inundar nó com conexões ou NACKs | Rate limiting: máx 10 novas conexões/s por IP. Máx 100 conexões simultâneas totais. Blacklist temporária (60s) após 5 tentativas de conexão rejeitadas. |
| **Extração de chave privada** | Acesso ao arquivo de chave no disco | Permissões de arquivo restritas. Aviso explícito se permissões estiverem erradas. Chave nunca serializada em logs ou mensagens de rede. |
| **Bootstrap envenenado** | Bootstrap nodes comprometidos fornecendo peer list maliciosa | Lista de bootstrap embutida no binário (não baixada em runtime). Verificar NodeRecord de cada peer descoberto antes de adicionar à routing table. |
| **Chunk com timestamp futuro** | Nó malicioso envia chunk com timestamp no futuro para causar dessincronia | Rejeitar chunks com `timestamp_ms > now + 5_000` (margem de 5s para clock skew). |

---

## Estrutura de Arquivos

```
prism/
├── Cargo.toml                           # workspace root
├── proto/
│   ├── node_record.proto
│   └── chunk_transfer.proto
└── crates/
    ├── prism-core/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── identity.rs              # Ed25519: keygen, sign, verify, NodeId
    │       ├── hash.rs                  # SHA-256 wrappers
    │       └── peer_reputation.rs       # Mapa de reputação por node_id: penalidade + ban
    ├── prism-proto/
    │   ├── Cargo.toml
    │   ├── build.rs                     # prost-build compila os .proto
    │   └── src/lib.rs
    └── prism-node/
        ├── Cargo.toml
        └── src/
            ├── main.rs
            ├── config.rs                # CLI args (clap) + config.toml
            ├── dht/
            │   ├── mod.rs
            │   └── record.rs            # NodeRecord: build, sign, verify, publish, renovar
            ├── transport/
            │   ├── mod.rs
            │   ├── quic.rs              # libp2p Swarm com QUIC + Noise XX
            │   └── rate_limit.rs        # conexões por IP, blacklist temporária
            ├── health/
            │   ├── mod.rs
            │   ├── benchmark.rs         # benchmark CPU + banda → CapacityReport
            │   └── vnodes.rs            # weight e vnodes_count
            └── transfer/
                ├── mod.rs
                └── chunk.rs             # send/receive com verificação completa + penalidade
```

---

## Cargo.toml

### Workspace root
```toml
[workspace]
members = ["crates/prism-core", "crates/prism-proto", "crates/prism-node"]
resolver = "2"
```

### `prism-core/Cargo.toml`
```toml
[package]
name = "prism-core"
version = "0.1.0"
edition = "2021"

[dependencies]
ed25519-dalek = { version = "2", features = ["rand_core"] }
rand          = "0.8"
sha2          = "0.10"
zeroize       = { version = "1", features = ["derive"] }
hex           = "0.4"
thiserror     = "1"
serde         = { version = "1", features = ["derive"] }
dashmap       = "5"      # PeerReputation: mapa concorrente node_id → score
```

### `prism-proto/Cargo.toml`
```toml
[package]
name = "prism-proto"
version = "0.1.0"
edition = "2021"

[dependencies]
prost = "0.13"

[build-dependencies]
prost-build = "0.13"
```

### `prism-node/Cargo.toml`
```toml
[package]
name = "prism-node"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core  = { path = "../prism-core" }
prism-proto = { path = "../prism-proto" }
tokio       = { version = "1", features = ["full"] }
libp2p      = { version = "0.54", features = [
    "kad", "quic", "noise", "yamux", "identify", "macros", "tokio"
] }
prost       = "0.13"
clap        = { version = "4", features = ["derive"] }
tracing     = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
anyhow      = "1"
thiserror   = "1"
serde       = { version = "1", features = ["derive"] }
toml        = "0.8"
```

---

## Schemas Protobuf

### `proto/node_record.proto`
```protobuf
syntax = "proto3";
package prism;

message NodeRecord {
  bytes  node_id         = 1; // SHA-256(pubkey) — 32 bytes — imutável
  bytes  pubkey          = 2; // Ed25519 verifying key — 32 bytes
  string region          = 3; // "sa-east-1", "us-east-1", etc.
  string capacity_class  = 4; // "A" | "B" | "C" | "edge"
  uint32 available_slots = 5;
  float  health_score    = 6; // 0.0–1.0
  uint64 expires_at_ms   = 7; // Unix ms — now + 10_000. Receptor rejeita se expirado.
  bytes  signature       = 8; // Ed25519Sign(privkey, sha256(campos 1–7 concatenados))
}
```

### `proto/chunk_transfer.proto`
```protobuf
syntax = "proto3";
package prism;

message ChunkTransfer {
  bytes  sender_node_id = 1; // SHA-256(sender pubkey) — 32 bytes
  bytes  sender_pubkey  = 2; // Ed25519 pubkey — 32 bytes (necessário para verificar sig)
  bytes  payload        = 3; // dados arbitrários
  bytes  payload_hash   = 4; // SHA-256(payload) — verificado antes de processar payload
  bytes  signature      = 5; // Ed25519Sign(privkey, payload_hash)
  uint64 timestamp_ms   = 6; // Unix ms — rejeitado se |now - ts| > 60_000
}
```

> **Nota de segurança:** `sender_pubkey` é incluído no proto para permitir verificação de assinatura sem estado prévio. O receptor deve verificar que `SHA-256(sender_pubkey) == sender_node_id` antes de usar a chave para verificar a assinatura.

---

## Interfaces dos Módulos

### `prism-core/src/identity.rs`

```rust
use ed25519_dalek::{SigningKey, VerifyingKey, Signature};

pub struct Identity {
    signing_key:       SigningKey,       // NUNCA exposta fora deste módulo
    pub verifying_key: VerifyingKey,
    pub node_id:       [u8; 32],        // SHA-256(verifying_key.as_bytes()) — cacheado
}

impl Identity {
    /// Gera novo par via OsRng. OBRIGATÓRIO: nunca usar thread_rng() ou SmallRng.
    pub fn generate() -> Self;

    /// Carrega chave de arquivo. Retorna Err se:
    ///   - arquivo não existe
    ///   - arquivo corrompido / tamanho errado
    ///   - permissões de arquivo permitem leitura por outros usuários (warn, não Err)
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self>;

    /// Persiste chave privada em disco.
    /// Unix: aplica chmod(600). Windows: remove ACEs de outros usuários.
    /// Retorna Err se não conseguir restringir permissões.
    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()>;

    /// Assina dados arbitrários. Nunca falha nem pânica.
    pub fn sign(&self, data: &[u8]) -> Signature;

    /// Verifica assinatura. Retorna false se inválida.
    /// NUNCA panic. NUNCA Err. Caller interpreta false como "rejeitar".
    pub fn verify(data: &[u8], sig: &Signature, pubkey: &VerifyingKey) -> bool;

    /// Hex string de 64 chars da chave pública. Seguro para logs.
    pub fn pubkey_hex(&self) -> String;
}
```

**Restrições de segurança:**
- A chave privada NUNCA deve aparecer em logs, mensagens de erro ou payloads de rede
- `save()` deve falhar explicitamente se não conseguir aplicar permissões restritivas
- `load()` deve emitir `tracing::warn!` se o arquivo tiver permissões abertas (world-readable)

### `prism-core/src/peer_reputation.rs`

```rust
pub struct PeerReputation {
    // DashMap<[u8; 32], ReputationEntry> — node_id → score
}

pub struct ReputationEntry {
    pub score:           i32,       // começa em 0; penalidades reduzem; máx 100
    pub ban_until_ms:    u64,       // 0 = não banido
    pub invalid_count:   u32,       // contador de eventos inválidos
}

impl PeerReputation {
    pub fn new() -> Self;

    /// Registra evento inválido. Reduz score em `penalty`.
    /// Se score < -50: aplica ban de 60s e emite tracing::warn!.
    pub fn penalize(&self, node_id: &[u8; 32], penalty: i32, reason: &str);

    /// Aumenta score (bom comportamento). Máx: 100.
    pub fn reward(&self, node_id: &[u8; 32], reward: i32);

    /// Retorna true se o nó está banido no momento atual.
    pub fn is_banned(&self, node_id: &[u8; 32]) -> bool;

    /// Retorna score atual. None se nunca visto.
    pub fn score(&self, node_id: &[u8; 32]) -> Option<i32>;
}
```

**Parâmetros de penalidade:**
| Evento | Penalidade |
|--------|-----------|
| Assinatura inválida em ChunkTransfer | -25 |
| NodeRecord com assinatura inválida | -20 |
| Timestamp fora do range (replay) | -15 |
| node_id ≠ SHA-256(pubkey) | -30 (indica forjamento de identidade) |
| Protobuf malformado | -10 |
| Chunk com payload_hash inválido | -25 |
| 5+ erros de conexão consecutivos | Ban 300s |

### `prism-node/src/transport/rate_limit.rs`

```rust
pub struct ConnectionRateLimiter {
    // por IP: sliding window de 10s
}

impl ConnectionRateLimiter {
    /// Retorna true se nova conexão do IP é permitida.
    /// Regras:
    ///   - Máx 10 novas conexões por IP por janela de 10s
    ///   - Máx 5 IPs distintos do mesmo /24 subnet
    ///   - IPs com is_banned = true: rejeitar imediatamente
    pub fn allow_connection(&self, ip: std::net::IpAddr) -> bool;

    /// Registra falha de autenticação para o IP. Após 5: ban de 60s.
    pub fn record_auth_failure(&self, ip: std::net::IpAddr);
}
```

### `prism-node/src/dht/record.rs`

```rust
pub struct DhtRecordManager { /* ... */ }

impl DhtRecordManager {
    pub fn new(identity: Arc<Identity>, capacity: CapacityReport) -> Self;

    /// Constrói, assina e publica NodeRecord na DHT.
    pub async fn publish_once(&self, kad: &mut Behaviour) -> anyhow::Result<()>;

    /// Loop em background: renova a cada 10s.
    /// Só republica se |Δweight| > 0.1 (evita flooding).
    pub fn start_renewal_loop(self: Arc<Self>, swarm: Arc<Mutex<Swarm<Behaviour>>>);

    /// Recebe NodeRecord da DHT e valida ANTES de usar.
    /// Retorna Err se qualquer verificação falhar.
    pub fn verify_incoming_record(record: &NodeRecord) -> anyhow::Result<()>;
}
```

**`verify_incoming_record` deve verificar na ordem:**
1. `record.node_id == SHA-256(record.pubkey)` → falha: `"node_id/pubkey mismatch — identity forging attempt"`
2. `record.expires_at_ms > now_unix_ms()` → falha: `"record expired"`
3. `record.expires_at_ms <= now_unix_ms() + 15_000` → falha: `"TTL too long — capping not allowed"` (evita poluição da DHT com registros de longa duração)
4. Assinatura Ed25519 válida → falha: `"invalid NodeRecord signature"`
5. `capacity_class` é um dos valores válidos: "A", "B", "C", "edge" → falha: `"unknown capacity class"`

**Chave DHT:** `sha256(b"prism:node:" || node_id)` → valor = `NodeRecord` serializado em Protobuf.

### `prism-node/src/transfer/chunk.rs`

```rust
pub enum ChunkError {
    ProtobufDeserializationFailed(String),
    NodeIdPubkeyMismatch,           // sender_node_id ≠ SHA-256(sender_pubkey)
    PayloadHashMismatch,            // SHA-256(payload) ≠ payload_hash
    InvalidSignature,               // Ed25519 inválida
    TimestampOutOfRange { delta_ms: i64 }, // |now - ts| > 60_000
    PeerBanned,                     // peer está na blacklist de reputação
}

/// Serializa, assina e envia ChunkTransfer via stream QUIC.
pub async fn send_chunk(
    stream:   &mut libp2p::Stream,
    identity: &Identity,
    payload:  Vec<u8>,
) -> anyhow::Result<()>;

/// Recebe e verifica ChunkTransfer.
/// Em caso de Err: caller DEVE chamar reputation.penalize(sender_node_id, ...) e logar.
/// Retorna (sender_node_id, payload) se válido.
pub async fn receive_chunk(
    stream:     &mut libp2p::Stream,
    reputation: &PeerReputation,
) -> Result<([u8; 32], Vec<u8>), ChunkError>;
```

**Verificações obrigatórias em `receive_chunk` (ordem estrita):**

```
1. Verificar se sender já está banido (reputation.is_banned)
   → Se banido: retornar Err(ChunkError::PeerBanned) imediatamente, sem ler mais dados

2. Deserializar Protobuf
   → Falha: penalize(-10, "malformed protobuf") + Err(ProtobufDeserializationFailed)

3. Verificar sender_node_id == SHA-256(sender_pubkey)
   → Falha: penalize(-30, "identity forging") + Err(NodeIdPubkeyMismatch)

4. Verificar SHA-256(payload) == payload_hash
   → Falha: penalize(-25, "payload hash mismatch") + Err(PayloadHashMismatch)

5. Verificar assinatura Ed25519: Identity::verify(payload_hash, signature, sender_pubkey)
   → Falha: penalize(-25, "invalid signature") + Err(InvalidSignature)

6. Verificar |now_unix_ms() - timestamp_ms| <= 60_000
   → Falha: penalize(-15, "timestamp out of range") + Err(TimestampOutOfRange { delta_ms })

7. Se todas as verificações passaram: reputation.reward(sender_node_id, 1)
   Retornar Ok((sender_node_id, payload))
```

---

## Benchmark de Capacidade

### `prism-node/src/health/benchmark.rs`

```rust
pub struct CapacityReport {
    pub cpu_score:      f32,    // 0.0–1.0
    pub bandwidth_mbps: f32,    // upload estimado em Mbps
    pub weight:         f32,    // min(cpu_score, bw_score) normalizado
    pub vnodes_count:   u32,    // floor(weight × 100), mín 1
    pub capacity_class: String, // "A" | "B" | "C" | "edge"
}

/// Executa benchmark. Duração máxima: 5s total.
/// Se config.capacity_class está definido, retorna CapacityReport com esse valor
/// e ignora benchmark (override manual).
pub async fn run_benchmark(config: &Config) -> CapacityReport;
```

**Lógica de classe (quando não há override):**
| Condição | Classe |
|----------|--------|
| `cpu_score < 0.3` e `bandwidth_mbps >= 10` | `"A"` |
| `cpu_score >= 0.5` | `"B"` |
| `bandwidth_mbps < 5` | `"C"` |
| Nenhuma das anteriores | `"edge"` |

**Cálculo de CPU score:** SHA-256 em loop por 2s, conta operações por segundo. Normaliza: `1_000_000 ops/s = score 1.0`. Clipa em [0.0, 1.0].

---

## Ordem de Implementação

```
1. prism-core/src/hash.rs               — sem dependências externas
2. prism-core/src/identity.rs           — depende de hash
3. prism-core/src/peer_reputation.rs    — sem dependências externas
4. prism-proto/                         — prost-build compila .proto
5. prism-node/src/config.rs             — CLI com clap
6. prism-node/src/transport/rate_limit.rs
7. prism-node/src/transport/quic.rs     — Swarm QUIC + Noise XX
8. prism-node/src/health/benchmark.rs
9. prism-node/src/health/vnodes.rs
10. prism-node/src/dht/record.rs        — depende de identity, quic
11. prism-node/src/transfer/chunk.rs    — depende de identity, reputation, quic
12. prism-node/src/main.rs              — orquestra tudo
```

---

## Testes

### Unitários

```rust
// prism-core/src/identity.rs

#[test]
fn sign_verify_roundtrip() {
    let id = Identity::generate();
    let sig = id.sign(b"hello prism");
    assert!(Identity::verify(b"hello prism", &sig, &id.verifying_key));
}

#[test]
fn verify_rejects_tampered_data() {
    let id = Identity::generate();
    let sig = id.sign(b"original");
    assert!(!Identity::verify(b"tampered", &sig, &id.verifying_key));
}

#[test]
fn verify_rejects_wrong_key() {
    let id_a = Identity::generate();
    let id_b = Identity::generate();
    let sig = id_a.sign(b"data");
    // assinatura de A não é válida com chave de B
    assert!(!Identity::verify(b"data", &sig, &id_b.verifying_key));
}

#[test]
fn save_load_preserves_node_id() {
    let id = Identity::generate();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    id.save(tmp.path()).unwrap();
    let loaded = Identity::load(tmp.path()).unwrap();
    assert_eq!(id.node_id, loaded.node_id);
    assert_eq!(id.pubkey_hex(), loaded.pubkey_hex());
}

#[cfg(unix)]
#[test]
fn save_applies_restricted_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let id = Identity::generate();
    let tmp = tempfile::NamedTempFile::new().unwrap();
    id.save(tmp.path()).unwrap();
    let mode = std::fs::metadata(tmp.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "file must be 0600");
}

// prism-core/src/peer_reputation.rs

#[test]
fn penalize_triggers_ban_at_threshold() {
    let rep = PeerReputation::new();
    let node_id = [1u8; 32];
    rep.penalize(&node_id, 25, "test");
    rep.penalize(&node_id, 25, "test");
    assert!(!rep.is_banned(&node_id)); // -50: ainda não banido
    rep.penalize(&node_id, 1, "test"); // -51: ban ativado
    assert!(rep.is_banned(&node_id));
}

#[test]
fn node_id_must_equal_sha256_of_pubkey() {
    let id = Identity::generate();
    use crate::hash::sha256;
    assert_eq!(id.node_id, sha256(id.verifying_key.as_bytes()));
}
```

```rust
// prism-node/src/dht/record.rs

#[test]
fn verify_record_rejects_mismatched_node_id() {
    let mut record = make_valid_node_record();
    record.node_id = vec![0u8; 32]; // adulterado — não bate com pubkey
    assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
}

#[test]
fn verify_record_rejects_expired() {
    let mut record = make_valid_node_record();
    record.expires_at_ms = now_unix_ms() - 1; // já expirou
    assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
}

#[test]
fn verify_record_rejects_ttl_too_long() {
    let mut record = make_valid_node_record();
    record.expires_at_ms = now_unix_ms() + 60_000; // 60s >> limite de 15s
    assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
}

#[test]
fn verify_record_rejects_invalid_signature() {
    let mut record = make_valid_node_record();
    record.health_score = 0.99; // adultera campo após assinar
    assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
}
```

### Integração (`tests/`)

**`tests/dht_discovery.rs` — 10 nós em localhost:**
```
1. Lançar 10 instâncias de prism-node em 127.0.0.1:4001–4010
   Todas apontam bootstrap para 127.0.0.1:4001
2. Aguardar 30s de convergência DHT
3. FIND_NODE(node_id_j) de cada nó i para todos j ≠ i
4. PASSA: todos os 90 lookups retornam NodeRecord com assinatura válida
```

**`tests/chunk_security.rs` — Matriz de ataques:**

| Caso | Adulteração | Resultado esperado |
|------|-------------|-------------------|
| E1 | Nenhuma (chunk válido) | `Ok(payload)` |
| E2 | 1 byte do payload | `Err(PayloadHashMismatch)` |
| E3 | node_id ≠ SHA-256(pubkey) | `Err(NodeIdPubkeyMismatch)` |
| E4 | Assinatura de outra chave | `Err(InvalidSignature)` |
| E5 | timestamp_ms = now - 120_000 (replay) | `Err(TimestampOutOfRange)` |
| E6 | timestamp_ms = now + 10_000 (futuro) | `Err(TimestampOutOfRange)` |
| E7 | Protobuf truncado (bytes faltando) | `Err(ProtobufDeserializationFailed)` |
| E8 | Peer banido (score < -50 antes) | `Err(PeerBanned)` imediato |

Cada caso deve ser testado 50× de forma automatizada. Nenhuma execução pode resultar em panic ou silêncio (Ok quando deveria ser Err).

**`tests/record_ttl.rs` — NodeRecord expira após queda:**
```
1. Nó A publica NodeRecord e então recebe SIGKILL
2. Aguardar 12s
3. Qualquer nó consultando DHT por node_id_A recebe "not found"
4. PASSA: DHT não retorna o registro expirado
```

**`tests/sybil_resistance.rs` — Rate limiting:**
```
1. Abrir 15 conexões simultâneas do mesmo IP para o nó alvo
2. PASSA: as primeiras 10 são aceitas; as 5 restantes são recusadas
3. Verificar que nó alvo não crashou e continua aceitando conexões de outros IPs
```

---

## Critérios de Conclusão

| # | Critério | Verificação |
|---|---------|-------------|
| C1 | `cargo test --workspace` sem falhas | CI |
| C2 | `cargo clippy --workspace -- -D warnings` sem warnings | CI |
| C3 | 10 nós se localizam via DHT em < 30s | Teste DHT discovery |
| C4 | Todos os 8 casos da matriz de ataques falham corretamente (50× cada) | Teste chunk_security |
| C5 | NodeRecord desaparece da DHT em ≤ 10s após SIGKILL | Teste record_ttl |
| C6 | Rate limiting rejeita excesso de conexões sem crash | Teste sybil_resistance |
| C7 | Nó em redes NAT distintas conecta em < 15s | Teste manual com 2 VMs |
| C8 | `prism-node` consome < 50 MB RAM com 10 peers conectados | `/proc/PID/status` ou `heaptrack` |
| C9 | Chave privada não aparece em nenhum log mesmo com `RUST_LOG=trace` | Inspeção manual de logs |
| C10 | Arquivo de chave com chmod 644 emite `warn!` no `load()` (Unix) | Teste automatizado |

---

## Fora do Escopo nesta Fase

- Vídeo, AV1, HLS, SVT-AV1 → Fase 2
- Chat, gossip, vector clocks → Fase 3
- UI: Studio, Player → Fase 2+
- Reed-Solomon, classes de nós A/B/C/edge → Fase 2
- State channels → Fase 4
