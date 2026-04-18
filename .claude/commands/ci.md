# /ci — Executar CI Local Completo

Executa a suite completa de verificações que o GitHub Actions executaria. Todos os itens devem passar antes de considerar a fase concluída.

## Sequência de Execução

Execute cada passo na ordem. Se algum falhar, reportar o erro e **não continuar** para o próximo passo.

### Passo 1 — Compilação do workspace Rust

```bash
cargo build --workspace 2>&1
```

Esperado: `Finished` sem erros. Warnings são permitidos aqui (o próximo passo os trata).

### Passo 2 — Clippy (zero warnings)

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

Esperado: saída sem `warning:` ou `error:`. Se houver warnings, são tratados como erros.  
**Não prosseguir se este passo falhar.**

### Passo 3 — Testes Rust

```bash
cargo test --workspace 2>&1
```

Reportar quantos testes passaram/falharam por crate.

### Passo 4 — Proto build (se proto/ existe)

```bash
# Verificar se os .proto foram compilados corretamente
cargo build -p prism-proto 2>&1
```

### Passo 5 — Frontend Studio (se crates/prism-studio/src existe)

```bash
cd crates/prism-studio && npm run build 2>&1
```

### Passo 6 — Frontend Player (se crates/prism-player/src existe)

```bash
cd crates/prism-player && npm run build 2>&1
```

### Passo 7 — Testes TypeScript (se package.json tem script "test")

```bash
cd crates/prism-studio && npm test -- --run 2>&1
cd crates/prism-player && npm test -- --run 2>&1
```

### Passo 8 — Auditoria de segurança rápida

Executar as verificações da Categoria 1 (crítico) do `/audit-seguranca`:

```bash
grep -rn "pub signing_key\|pub privkey" crates/ --include="*.rs"
grep -rn "println!" crates/ --include="*.rs" | grep -v "cfg(test)"
grep -rn "\.unwrap()" crates/ --include="*.rs" | grep -v "cfg(test)" | grep -v "//.*ok"
```

Se alguma ocorrência crítica for encontrada, reportar como falha do CI.

## Relatório Final

```
CI LOCAL — PRISM
================

Passo 1 — cargo build:     [PASS | FAIL]
Passo 2 — cargo clippy:    [PASS | FAIL] (N warnings tratados como erros)
Passo 3 — cargo test:      [PASS | FAIL] (N passed, M failed)
  prism-core:     X/X
  prism-proto:    X/X
  prism-node:     X/X
  prism-encoder:  X/X  (se existir)
  prism-chat:     X/X  (se existir)
  ...
Passo 4 — proto build:     [PASS | FAIL | SKIP]
Passo 5 — studio build:    [PASS | FAIL | SKIP]
Passo 6 — player build:    [PASS | FAIL | SKIP]
Passo 7 — ts tests:        [PASS | FAIL | SKIP]
Passo 8 — security check:  [PASS | FAIL] (N violações críticas)

RESULTADO GERAL: [PASS | FAIL]

[Se FAIL: listar cada item falhando com a causa]
```

## Modo Rápido

Se o argumento for `--quick` (ex: `/ci --quick`), executar apenas os passos 2 e 3 (clippy + testes Rust). Útil durante desenvolvimento iterativo.
