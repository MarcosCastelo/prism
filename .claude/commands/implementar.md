# /implementar — Implementar Próximo Módulo do PRD

Argumento opcional: nome do módulo específico (ex: `/implementar prism-core/identity`).  
Sem argumento: implementar o próximo módulo pendente da fase atual.

## Instruções

### 1. Identificar o módulo alvo

Se argumento foi fornecido (`$ARGUMENTS`):
- O argumento é o caminho do módulo (ex: `prism-node/src/dht/record`)
- Localizar a seção correspondente no PRD da fase atual

Se nenhum argumento:
- Executar `/fase-atual` mentalmente para identificar o próximo módulo pendente
- Usar o primeiro item não implementado da seção "Ordem de Implementação" do PRD

### 2. Ler a especificação completa do módulo

Abrir o PRD da fase atual (`docs/prd/fase-X-*.md`) e ler:
- Interface Rust exata (assinaturas de função, tipos, enums)
- Comportamentos obrigatórios descritos em pseudocódigo
- Verificações de segurança aplicáveis ao módulo
- Testes que devem ser escritos

### 3. Verificar pré-condições

Confirmar que todos os módulos que este depende já existem (conforme "Ordem de Implementação" do PRD). Se alguma dependência estiver faltando, implementar a dependência primeiro.

### 4. Implementar

Seguindo estritamente a interface definida no PRD:

**Para módulos Rust:**
- Criar o arquivo na path exata definida no PRD
- Implementar **todas** as funções públicas listadas na interface
- Aplicar as regras de segurança do `CLAUDE.md` relevantes a este módulo
- Sem `unwrap()` em código de produção
- Sem `println!` — usar `tracing::info/warn/error`
- Erros: `thiserror` em library crates, `anyhow` em binaries

**Para módulos TypeScript/React:**
- Usar tipos explícitos (sem `any`)
- Async/await em vez de `.then()`
- Componentes funcionais com hooks

### 5. Escrever os testes

Após implementar o código principal, escrever os testes descritos no PRD para este módulo:
- Testes unitários no mesmo arquivo (módulo `#[cfg(test)]`)
- Todos os edge cases listados na tabela de testes do PRD

### 6. Verificar

```bash
# Para Rust:
cargo clippy -p <nome-da-crate> -- -D warnings
cargo test -p <nome-da-crate>

# Para TypeScript:
cd crates/<app> && npm run build && npm test
```

Se `clippy` retornar warnings: corrigir antes de continuar.  
Se testes falharem: corrigir antes de marcar como concluído.

### 7. Reportar

Ao finalizar, reportar:
```
IMPLEMENTADO: crates/<caminho>/<arquivo>.rs
INTERFACE: [lista das funções públicas implementadas]
TESTES: X passando, 0 falhando
PRÓXIMO MÓDULO: [conforme Ordem de Implementação do PRD]
```

## Regras de Segurança Sempre Aplicadas

Independente do módulo, sempre:

1. Se o módulo recebe dados de rede → verificar assinatura Ed25519 antes de processar
2. Se o módulo lida com chaves → nunca expor `signing_key` fora do módulo
3. Se detectar dado inválido → chamar `reputation.penalize()` e retornar `Err`
4. Se o módulo usa timestamp → validar `|now - ts| <= 60_000 ms`
5. Geração de chaves → usar `OsRng`, nunca `thread_rng()`

Referência completa: `CLAUDE.md` seção "Regras de Segurança".
