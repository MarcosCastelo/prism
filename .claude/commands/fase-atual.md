# /fase-atual — Determinar Fase Atual e Próxima Ação

Você deve determinar em que fase de desenvolvimento o projeto Prism está e reportar o próximo passo concreto a ser executado.

## Instruções

Execute estas verificações na ordem e reporte o resultado de cada uma:

### 1. Verificar estrutura do workspace

```bash
ls -la crates/ 2>/dev/null || echo "crates/ não existe"
ls crates/ 2>/dev/null
```

### 2. Verificar crates existentes vs esperadas por fase

**Fase 1 concluída se** todos estes existem com testes passando:
- `crates/prism-core/src/identity.rs`
- `crates/prism-core/src/hash.rs`
- `crates/prism-core/src/peer_reputation.rs`
- `crates/prism-proto/build.rs`
- `crates/prism-node/src/dht/record.rs`
- `crates/prism-node/src/transport/quic.rs`
- `crates/prism-node/src/health/benchmark.rs`
- `crates/prism-node/src/transfer/chunk.rs`

**Fase 2 concluída se** todos acima + estes existem:
- `crates/prism-encoder/src/svt_av1.rs`
- `crates/prism-ingest/src/injector.rs`
- `crates/prism-node/src/relay/tree.rs`
- `crates/prism-node/src/classes/class_a.rs`
- `crates/prism-node/src/erasure/reed_solomon.rs`
- `crates/prism-studio/src-tauri/src/main.rs`
- `crates/prism-player/src/player/HlsPlayer.tsx`

**Fase 3 concluída se** todos acima + estes existem:
- `crates/prism-chat/src/gossip.rs`
- `crates/prism-chat/src/vector_clock.rs`
- `crates/prism-chat/src/anchor.rs`
- `crates/prism-chat/src/moderation.rs`
- `crates/prism-player/src/identity/useIdentity.ts`

**Fase 4 em andamento se** todos acima existem.

### 3. Executar testes da fase identificada

```bash
cargo test --workspace 2>&1 | tail -30
```

### 4. Identificar próximo entregável

Com base na fase identificada, ler o PRD correspondente:
- Fase 1 → `docs/prd/fase-1-fundacao-p2p.md`
- Fase 2 → `docs/prd/fase-2-pipeline-video.md`
- Fase 3 → `docs/prd/fase-3-chat-identidade.md`
- Fase 4 → `docs/prd/fase-4-producao-resiliencia.md`

Na seção **"Ordem de Implementação"** do PRD, identificar o primeiro item que ainda não tem arquivo correspondente criado.

## Output Esperado

Retorne um relatório com este formato:

```
FASE ATUAL: [1|2|3|4]
STATUS: [Em andamento | Concluída]

CRATES EXISTENTES: [lista]
CRATES FALTANDO: [lista do que falta para concluir a fase]

PRÓXIMO PASSO:
  Módulo: [caminho/do/arquivo.rs ou tsx]
  PRD ref: docs/prd/fase-X.md → seção "Ordem de Implementação" item N
  Ação: [descrição específica do que implementar]

TESTES:
  Passando: X
  Falhando: Y
  [se Y > 0: listar os testes falhando]
```

Não inicie a implementação — apenas reporte o estado.
