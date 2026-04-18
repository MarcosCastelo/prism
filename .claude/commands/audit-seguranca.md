# /audit-seguranca — Auditoria de Segurança do Código

Argumento opcional: caminho de uma crate ou arquivo específico (ex: `/audit-seguranca crates/prism-node`).  
Sem argumento: auditar todo o workspace.

## O que Auditar

Executar as buscas abaixo e reportar todas as ocorrências encontradas.

### Categoria 1 — Chave Privada Exposta (CRÍTICO)

```bash
# Buscar por possíveis exposições de chave privada em logs
grep -rn "signing_key" crates/ --include="*.rs" | grep -v "self\." | grep -v "let signing_key" | grep -v "//.*signing_key"
grep -rn "privkey\|private_key\|secret_key" crates/ --include="*.rs" | grep -E "log|print|format|debug|info|warn|error"
grep -rn "pub signing_key\|pub privkey\|pub private_key" crates/ --include="*.rs"
```

**Qualquer ocorrência acima é uma violação crítica.**

### Categoria 2 — Verificação de Assinatura Ausente (ALTO)

Para cada arquivo que contém `receive_chunk`, `validate_incoming`, `on_receive`, ou que deserializa Protobuf de rede:

```bash
grep -rn "decode\|from_bytes\|Protobuf\|prost::Message" crates/ --include="*.rs" -l
```

Para cada arquivo retornado, verificar se **antes** do uso dos dados há:
- `Identity::verify(` ou equivalente
- `sha256(` para verificar hash de payload
- Verificação de `timestamp_ms`

Se algum arquivo deserializa dados externos sem verificar assinatura → reportar como violação.

### Categoria 3 — Código Inseguro de Produção (MÉDIO)

```bash
# unwrap() fora de testes
grep -rn "\.unwrap()" crates/ --include="*.rs" | grep -v "#\[cfg(test)\]" | grep -v "//.*unwrap"

# expect() sem mensagem descritiva (apenas "expect()")
grep -rn '\.expect("")\|\.expect("TODO")' crates/ --include="*.rs"

# println! em código de produção (não em testes)
grep -rn "println!" crates/ --include="*.rs" | grep -v "#\[cfg(test)\]" | grep -v "//.*println"

# thread_rng() para criptografia
grep -rn "thread_rng\|SmallRng\|StdRng::from_entropy" crates/ --include="*.rs"
```

### Categoria 4 — Penalidade de Reputação Ausente (MÉDIO)

Buscar funções que retornam `Err` após detectar dado inválido sem chamar `penalize`:

```bash
grep -rn "invalid.*signature\|hash.*mismatch\|invalid.*pubkey\|forging" crates/ --include="*.rs" -A 3 | grep -v "penalize\|reputation"
```

Se um `Err(ChunkError::InvalidSignature)` ou similar é retornado sem um `reputation.penalize()` antes → reportar.

### Categoria 5 — RS(10,4) Descrito Incorretamente (BAIXO)

```bash
grep -rn "qualquer 4\|any 4.*fragment\|4.*reconstruct" crates/ --include="*.rs" docs/ --include="*.md"
```

Qualquer texto que diga "qualquer 4 fragmentos reconstroem" está errado — deve ser "quaisquer 10 de 14".

### Categoria 6 — Permissões de Arquivo de Chave (MÉDIO)

```bash
grep -rn "save\|persist\|write.*key\|store.*key" crates/prism-core/src/identity.rs
```

Verificar se a função de `save()` da identidade aplica `chmod 600` no Unix e ACL restritiva no Windows. Se não encontrar chamada a `set_permissions` ou equivalente → reportar.

### Categoria 7 — Segurança do Frontend TypeScript (BAIXO)

```bash
grep -rn "any\b" crates/prism-player/src crates/prism-studio/src --include="*.ts" --include="*.tsx" | grep -v "//.*any" | grep -v "eslint-disable"
grep -rn "localStorage.*private\|sessionStorage.*private\|window\.\w*key" crates/ --include="*.ts" --include="*.tsx"
```

`any` é aceitável se comentado como intencional. Chave privada nunca deve ir para `localStorage` ou `sessionStorage`.

## Formato do Relatório

```
AUDITORIA DE SEGURANÇA — PRISM
Escopo: [workspace | crate específica | arquivo]
Data: [data]

CRÍTICO (resolver imediatamente):
  [arquivo:linha] — [descrição da violação]

ALTO (resolver antes de avançar de fase):
  [arquivo:linha] — [descrição da violação]

MÉDIO (resolver antes de lançamento):
  [arquivo:linha] — [descrição da violação]

BAIXO (melhorias):
  [arquivo:linha] — [descrição da violação]

RESUMO:
  Crítico: X
  Alto: Y
  Médio: Z
  Baixo: W
  Total de arquivos auditados: N
```

Se nenhuma violação for encontrada em uma categoria, omiti-la do relatório.

## Correção Automática

Para violações de Categoria 3 (`unwrap()`, `println!`):
- Se o argumento incluiu `--fix` (ex: `/audit-seguranca --fix`), corrigir automaticamente:
  - `x.unwrap()` → `x?` ou `x.expect("invariant: <razão>")`
  - `println!(...)` → `tracing::info!(...)`
- Para violações de Categoria 1 e 2: **não corrigir automaticamente** — reportar e aguardar instrução.
