# /proto — Compilar Protobufs e Sincronizar Código Gerado

Argumento opcional: nome de um `.proto` específico (ex: `/proto video_chunk`).  
Sem argumento: compilar todos os `.proto` em `proto/`.

## Instruções

### 1. Verificar arquivos .proto existentes

```bash
ls proto/*.proto 2>/dev/null || echo "Nenhum .proto encontrado em proto/"
```

### 2. Verificar se prism-proto/build.rs está correto

Ler `crates/prism-proto/build.rs` e confirmar que ele lista todos os `.proto` de `proto/`.

Se um `.proto` em `proto/` não está no `build.rs`, adicioná-lo:

```rust
// build.rs padrão esperado:
fn main() -> anyhow::Result<(), Box<dyn std::error::Error>> {
    prost_build::compile_protos(
        &[
            "../../proto/node_record.proto",
            "../../proto/chunk_transfer.proto",
            // ... todos os .proto
        ],
        &["../../proto/"],
    )?;
    Ok(())
}
```

### 3. Compilar

```bash
cargo build -p prism-proto 2>&1
```

Se falhar por schema inválido no `.proto`:
- Ler a mensagem de erro
- Identificar o campo problemático
- Corrigir o `.proto` (não o código gerado)
- Recompilar

### 4. Verificar código gerado

```bash
# O código gerado fica em $OUT_DIR — verificar se compila sem erros
cargo check -p prism-proto 2>&1
```

### 5. Verificar consistência com os PRDs

Para cada mensagem Protobuf gerada, confirmar que os campos correspondem exatamente ao que está definido nos PRDs em `docs/prd/`:

- `VideoChunk` → `docs/prd/fase-2-pipeline-video.md` seção "proto/video_chunk.proto"
- `ChatMessage` → `docs/prd/fase-3-chat-identidade.md` seção "proto/chat_message.proto"
- `NodeRecord` → `docs/prd/fase-1-fundacao-p2p.md` seção "proto/node_record.proto"
- etc.

Se houver divergência entre o `.proto` e o PRD, reportar qual é a discrepância e aguardar instrução antes de alterar.

### 6. Verificar uso em outros crates

```bash
# Garantir que crates que usam prism-proto ainda compilam
cargo check --workspace 2>&1 | grep -E "error\[|prism-proto"
```

## Relatório

```
PROTO SYNC — PRISM
==================

Arquivos .proto encontrados:
  proto/node_record.proto      [presente no build.rs: Sim/Não]
  proto/video_chunk.proto      [presente no build.rs: Sim/Não]
  ...

Compilação prism-proto:  [PASS | FAIL]
Check workspace:         [PASS | FAIL]

Consistência com PRDs:
  NodeRecord:    [OK | DIVERGÊNCIA: descrever]
  VideoChunk:    [OK | DIVERGÊNCIA: descrever]
  ChatMessage:   [OK | DIVERGÊNCIA: descrever]
  ...

[Se houver divergências: listar e aguardar instrução]
```

## Adicionar Novo .proto

Se o argumento for o nome de um novo proto (ex: `/proto payment_channel`):

1. Criar `proto/payment_channel.proto` com o schema definido no PRD da fase atual
2. Adicionar ao `build.rs` do `prism-proto`
3. Compilar e verificar
4. Reportar as structs Rust geradas que o agente pode usar
