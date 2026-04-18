# Guia de Desenvolvimento com Agentes de Código — Prism

Este guia explica como usar o Claude Code e as skills criadas para implementar o Prism de forma eficiente e segura.

## Pré-requisitos

- [Claude Code](https://claude.ai/code) instalado (`npm install -g @anthropic-ai/claude-code` ou via app desktop)
- Rust toolchain estável (`rustup update stable`)
- Node.js 20+ (para crates com frontend Tauri)
- Git com acesso ao repositório

## Estrutura de Suporte ao Agente

```
openstream/
├── CLAUDE.md                    # Regras, padrões e fluxo — lido automaticamente
├── .claude/
│   ├── settings.json            # Permissões de ferramentas e hooks
│   └── commands/                # Skills invocáveis com /nome
│       ├── fase-atual.md        # Detecta fase e próximo passo
│       ├── implementar.md       # Implementa um módulo do PRD
│       ├── ci.md                # CI local completo
│       ├── testar-fase.md       # Testes de aceitação da fase
│       ├── check-prd.md         # Verifica conformidade código ↔ PRD
│       ├── audit-seguranca.md   # Auditoria de vulnerabilidades
│       ├── proto.md             # Compila/sincroniza Protobufs
│       └── novo-crate.md        # Cria nova crate no workspace
└── docs/prd/                    # Fonte de verdade para implementação
```

## Sessão Típica de Desenvolvimento

### 1. Iniciar uma sessão

Abra o Claude Code na raiz do repositório. O `CLAUDE.md` é carregado automaticamente — o agente já conhece o projeto.

Primeiro comando sempre:

```
/fase-atual
```

Saída esperada:
```
FASE ATUAL: Fase 1 — Fundação P2P
Progresso: 3/7 arquivos esperados presentes
Próximo passo: implementar crates/prism-node/src/transfer/chunk.rs
Referência PRD: docs/prd/fase-1-fundacao-p2p.md §4 "Transferência de Chunks"
```

### 2. Implementar um módulo

Com base na saída do `/fase-atual`, implementar o próximo módulo:

```
/implementar prism-node/src/transfer/chunk.rs
```

O agente irá:
1. Ler a seção correspondente em `docs/prd/fase-1-fundacao-p2p.md`
2. Criar o arquivo com as assinaturas exatas definidas no PRD
3. Implementar a lógica seguindo as regras de segurança do `CLAUDE.md`
4. Executar `cargo clippy` e `cargo test` antes de marcar como concluído

### 3. Verificar conformidade com o PRD

Após implementar um conjunto de módulos:

```
/check-prd
```

Verifica se as assinaturas públicas, a ordem de verificações de segurança e os testes listados no PRD estão todos presentes no código. Exemplo de saída:

```
CHECK-PRD — PRISM Fase 1
  Interfaces: 12/15 conformes
  Segurança: 3/4 conformes
  Testes: 8/12 presentes
  AÇÕES: adicionar ChunkError::PeerBanned, reordenar verify_record
```

Para corrigir automaticamente os problemas encontrados:

```
/check-prd --fix
```

### 4. Rodar CI local

Antes de avançar de fase ou abrir PR:

```
/ci
```

Executa em sequência: compilação → clippy -D warnings → testes → proto build → builds frontend → auditoria de segurança. Para no primeiro falho.

Modo rápido durante desenvolvimento iterativo:

```
/ci --quick
```

Executa apenas clippy + testes Rust.

### 5. Rodar testes de aceitação da fase

```
/testar-fase
```

Executa os critérios de conclusão definidos no PRD da fase atual. Separa os automáticos (executados agora) dos manuais (documentados para execução humana). Exemplo:

```
TESTES DE ACEITAÇÃO — PRISM Fase 1
  C1: cargo test --workspace       [PASS]
  C2: cargo clippy -D warnings     [PASS]
  C4: chunk corrompido rejeitado   [PASS]
  C5: NodeRecord TTL               [PASS]
  C6: rate limiting                [FAIL — teste não encontrado]

  MANUAIS:
  C7: dois nós em NAT distintas → executar com duas VMs
  C8: memória < 50MB com 10 peers → medir com heaptrack

  RESULTADO: 4/6 automáticos passando
  FASE CONCLUÍDA: Não
```

Para corrigir implementações incompletas que causam falhas:

```
/testar-fase --fix
```

### 6. Auditoria de segurança

Execute antes de fechar qualquer fase:

```
/audit-seguranca
```

Procura por: chave privada exposta em logs, verificações de segurança ausentes, `unwrap()` em código de produção, `println!` fora de testes, uso de `thread_rng` em vez de `OsRng`. Exemplo de saída com severidade:

```
[CRITICAL] crates/prism-core/src/identity.rs:45 — signing_key em campo pub
[HIGH]     crates/prism-node/src/dht/record.rs — timestamp verificado antes de node_id
[MEDIUM]   crates/prism-node/src/relay.rs:12 — .unwrap() sem justificativa
```

Para corrigir automaticamente os problemas encontrados:

```
/audit-seguranca --fix
```

### 7. Trabalhar com Protobufs

Sempre que adicionar ou modificar um `.proto` em `proto/`:

```
/proto
```

Verifica se todos os `.proto` estão no `build.rs` do `prism-proto`, compila, e confirma que os campos correspondem às definições nos PRDs. Para adicionar um novo proto:

```
/proto payment_channel
```

### 8. Criar uma nova crate

Quando o PRD indica que uma nova crate deve ser iniciada:

```
/novo-crate prism-encoder
```

Cria `crates/prism-encoder/` com `Cargo.toml` (dependências do PRD), `src/lib.rs` (esqueleto com `thiserror`) e adiciona ao `members` do workspace raiz.

## Fluxo Recomendado por Fase

```
Início de fase:
  /fase-atual                    ← confirmar ponto de partida

Loop de implementação:
  /implementar <módulo>          ← implementar um módulo
  /ci --quick                    ← verificar imediatamente
  (repetir para cada módulo)

Antes de fechar a fase:
  /check-prd                     ← conformidade com o PRD
  /audit-seguranca               ← sem vulnerabilidades
  /testar-fase                   ← critérios de aceitação
  /ci                            ← CI completo final
```

## Permissões e Hooks

O arquivo `.claude/settings.json` define:

- **Permitido automaticamente**: `cargo`, `npm`, `git status/log/diff`, leitura de arquivos, compilação
- **Negado**: `rm -rf`, `curl`, `git push`, `git reset --hard`, comandos com `sudo`
- **Hook pós-escrita de `.rs`**: executa `cargo clippy` automaticamente após salvar qualquer arquivo Rust

Se o agente tentar uma ação negada, ele reportará o bloqueio e pedirá confirmação antes de prosseguir.

## Regras de Segurança que o Agente Sempre Aplica

Definidas no `CLAUDE.md` e aplicadas independente do módulo:

1. Chave privada nunca em logs, campos públicos ou mensagens de erro
2. Toda entrada externa verificada na ordem: banido → parse → node_id → hash → assinatura → timestamp
3. Penalidade de reputação aplicada em cada violação (tabela no `CLAUDE.md`)
4. `OsRng` para geração de chaves (nunca `thread_rng`)
5. RS(10,4) = 10 dados + 4 paridade = 14 fragmentos; reconstrução exige QUAISQUER 10 de 14

Se um módulo parecer não precisar dessas regras, o agente deve perguntar — provavelmente está faltando contexto.

## Dicas de Uso

**Contexto explícito é melhor que implícito.** Em vez de "implemente o módulo de chunks", prefira:

```
/implementar prism-node/src/transfer/chunk.rs
```

**Verificar antes de avançar.** Nunca passar para a próxima fase sem `/testar-fase` e `/audit-seguranca` passando.

**PRDs são a fonte de verdade.** Se o código divergir de um PRD, o PRD está certo — a não ser que haja instrução explícita para alterar o PRD primeiro.

**Testes manuais documentados.** Critérios que precisam de ambiente real (dois nós em NAT, 10.000 viewers) são documentados pelo `/testar-fase` com instruções de como executar — não são ignorados.

## Referências

- [CLAUDE.md](../CLAUDE.md) — regras completas para o agente
- [Fase 1 PRD](prd/fase-1-fundacao-p2p.md) — interfaces, testes, critérios
- [Fase 2 PRD](prd/fase-2-pipeline-video.md)
- [Fase 3 PRD](prd/fase-3-chat-identidade.md)
- [Fase 4 PRD](prd/fase-4-producao-resiliencia.md)
- [Arquitetura](architecture/overview.md)
