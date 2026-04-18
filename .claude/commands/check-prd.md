# /check-prd — Verificar Conformidade do Código com o PRD

Argumento opcional: fase específica (ex: `/check-prd fase-1`) ou crate (ex: `/check-prd prism-node`).  
Sem argumento: verificar a fase atual completa.

Verifica se o código implementado corresponde ao que foi especificado no PRD — interfaces, comportamentos, testes e critérios de conclusão.

## Instruções

### 1. Identificar escopo

Se argumento é uma fase (ex: `fase-2`): ler `docs/prd/fase-2-pipeline-video.md` completo.  
Se argumento é uma crate (ex: `prism-chat`): ler seções do PRD relevantes a essa crate.  
Sem argumento: usar a fase atual (detectada pelo mesmo método de `/fase-atual`).

### 2. Para cada interface definida no PRD

Para cada bloco de código Rust no PRD (funções `pub`, structs, enums):

a) Verificar se o arquivo existe no caminho especificado
b) Ler o arquivo e verificar se a assinatura pública corresponde ao PRD

**Exemplo de verificação:**
- PRD diz: `pub fn verify(data: &[u8], sig: &Signature, pubkey: &VerifyingKey) -> bool`
- Código tem: `pub fn verify(data: &[u8], sig: &Signature, pk: &VerifyingKey) -> bool` → nomes de parâmetros diferentes são OK
- Código tem: `pub fn verify(data: &[u8], sig: &Signature) -> bool` → parâmetro faltando → **DIVERGÊNCIA**

```bash
# Para cada arquivo Rust listado na estrutura do PRD:
grep -n "^pub fn\|^pub async fn\|^pub struct\|^pub enum" crates/<crate>/src/<modulo>.rs 2>/dev/null
```

### 3. Para cada verificação de segurança obrigatória no PRD

Verificar se o fluxo de verificação está implementado na ordem correta.

**Para `receive_chunk`:**
```bash
grep -n "is_banned\|node_id.*sha256\|payload_hash\|Identity::verify\|timestamp_ms" \
  crates/prism-node/src/transfer/chunk.rs 2>/dev/null
```
As verificações devem aparecer nesta ordem no código.

**Para `verify_incoming_record`:**
```bash
grep -n "sha256\|expires_at_ms\|Ed25519\|capacity_class" \
  crates/prism-node/src/dht/record.rs 2>/dev/null
```

### 4. Para cada teste listado no PRD

Verificar se o teste existe no código:

```bash
# Buscar por nome de teste do PRD
grep -rn "fn test_<nome>" crates/ --include="*.rs"
grep -rn "fn <nome>" crates/ --include="*.ts" --include="*.spec.ts"
```

Para cada teste listado no PRD que não foi encontrado no código → reportar como ausente.

### 5. Verificar Critérios de Conclusão

Para cada critério na tabela "Critérios de Conclusão" do PRD:

| Critério | Verificação possível |
|----------|---------------------|
| `cargo test --workspace` | Executar e verificar resultado |
| `cargo clippy -- -D warnings` | Executar e verificar resultado |
| Testes específicos de integração | Executar se existirem |
| Critérios manuais (NAT, VMs) | Marcar como "requer teste manual" |

```bash
cargo test --workspace 2>&1 | grep -E "test result|FAILED|error"
cargo clippy --workspace -- -D warnings 2>&1 | grep -c "warning:"
```

### 6. Gerar Relatório de Conformidade

```
CHECK-PRD — PRISM [Fase X]
==========================

INTERFACES (assinaturas de função):
  prism-core/src/identity.rs:
    ✓ Identity::generate()
    ✓ Identity::load()
    ✗ Identity::save() — PRD: save(&self, path) → implementado: save(&mut self, path) [tipo de self diferente]
    ✓ Identity::sign()
    ✓ Identity::verify()
  
  prism-node/src/transfer/chunk.rs:
    ✓ send_chunk()
    ✓ receive_chunk()
    ✗ ChunkError enum — PRD lista 8 variantes, código tem 6 [NodeIdPubkeyMismatch e PeerBanned ausentes]

ORDEM DE VERIFICAÇÕES DE SEGURANÇA:
  receive_chunk:   ✓ ordem correta (is_banned → protobuf → node_id check → hash → sig → timestamp)
  verify_record:   ✗ timestamp verificado antes do node_id (PRD exige node_id primeiro)

TESTES:
  Presentes no código:
    ✓ sign_verify_roundtrip
    ✓ verify_rejects_tampered_data
    ✗ verify_rejects_wrong_key — não encontrado
    ✓ save_load_preserves_node_id
  
  Ausentes (listados no PRD mas não encontrados):
    ✗ save_applies_restricted_permissions
    ✗ receive_rejects_corrupted_payload

CRITÉRIOS DE CONCLUSÃO:
  C1: cargo test --workspace:      PASS (47 passed, 0 failed)
  C2: cargo clippy -D warnings:    FAIL (3 warnings)
  C3: 10 nós DHT em < 30s:         REQUER TESTE MANUAL
  C4: Chunk corrompido rejeitado:   REQUER EXECUÇÃO DOS TESTES

RESUMO:
  Interfaces conformes:     12/15
  Verificações de segurança conformes: 3/4
  Testes presentes:         8/12
  Critérios automatizáveis passando: 1/2

AÇÕES NECESSÁRIAS ANTES DE AVANÇAR:
  1. Adicionar ChunkError::NodeIdPubkeyMismatch e ChunkError::PeerBanned
  2. Reordenar verificações em verify_record (node_id antes de timestamp)
  3. Escrever testes: verify_rejects_wrong_key, save_applies_restricted_permissions, receive_rejects_corrupted_payload
  4. Corrigir 3 clippy warnings
```

### 7. Correção Automática

Se o argumento incluir `--fix` (ex: `/check-prd fase-1 --fix`):
- Para divergências de testes ausentes: escrever os testes faltantes
- Para variantes de enum ausentes: adicionar as variantes e atualizar os match arms
- Para clippy warnings: corrigir
- Para reordenação de verificações de segurança: reordenar o código

**Não alterar interfaces públicas automaticamente** (mudanças de assinatura podem quebrar callers) — apenas reportar e aguardar instrução.
