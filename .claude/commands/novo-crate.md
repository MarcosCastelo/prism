# /novo-crate — Criar Nova Crate no Workspace

Argumento obrigatório: nome da crate (ex: `/novo-crate prism-payments`).

A crate deve ser uma das definidas nos PRDs. Não criar crates não previstas sem instrução explícita.

## Instruções

### 1. Validar o nome

Verificar que o nome fornecido (`$ARGUMENTS`) corresponde a uma crate listada em `CLAUDE.md` seção "Estrutura do Workspace Cargo". Se não corresponder, avisar e aguardar confirmação.

### 2. Verificar se já existe

```bash
ls crates/$ARGUMENTS 2>/dev/null && echo "já existe" || echo "não existe"
```

Se já existir: reportar e não sobrescrever.

### 3. Determinar as dependências da crate

Consultar o PRD da fase atual (`docs/prd/`) e localizar a seção `Cargo.toml` para esta crate. Usar exatamente as dependências listadas no PRD.

Se a crate não tiver seção dedicada de `Cargo.toml` no PRD, usar este template mínimo:

```toml
[package]
name = "<nome>"
version = "0.1.0"
edition = "2021"

[dependencies]
prism-core = { path = "../prism-core" }
anyhow     = "1"
thiserror  = "1"
tracing    = "0.1"
```

### 4. Criar estrutura de arquivos

```bash
mkdir -p crates/$ARGUMENTS/src
```

**`crates/<nome>/Cargo.toml`** — com as dependências do PRD ou template acima.

**`crates/<nome>/src/lib.rs`** — com este conteúdo inicial:

```rust
//! <nome> — parte do projeto Prism
//!
//! Consultar docs/prd/ para a especificação completa desta crate.

pub mod error {
    #[derive(thiserror::Error, Debug)]
    pub enum Error {
        #[error("not implemented: {0}")]
        NotImplemented(&'static str),
    }
}
```

Se a crate for um binário (ex: `prism-node`), criar também `src/main.rs`:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("prism-<nome> starting");
    Ok(())
}
```

### 5. Adicionar ao workspace

Ler `Cargo.toml` da raiz e adicionar a nova crate à lista de `members`:

```toml
[workspace]
members = [
    "crates/prism-core",
    "crates/prism-proto",
    # ... existentes ...
    "crates/<nome>",   # ← adicionar aqui em ordem alfabética
]
```

### 6. Verificar compilação

```bash
cargo check -p $ARGUMENTS 2>&1
```

Esperado: `Checking <nome> v0.1.0` sem erros.

### 7. Reportar

```
CRATE CRIADA: crates/<nome>/
  Cargo.toml:  criado com N dependências
  src/lib.rs:  criado (esqueleto)
  Workspace:   adicionado ao Cargo.toml raiz

cargo check: PASS

PRÓXIMO PASSO:
  Consultar docs/prd/<fase>.md seção "<nome>" para implementar os módulos.
  Executar /implementar <nome>/src/<primeiro-modulo> para começar.
```
