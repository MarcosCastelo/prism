# /testar-fase — Executar Testes de Aceitação da Fase Atual

Argumento opcional: fase específica (ex: `/testar-fase fase-2`).  
Sem argumento: testar a fase atual.

Executa os testes de integração definidos nos Critérios de Conclusão do PRD e reporta o resultado de cada um. Este comando vai além do `/ci` — executa testes específicos de comportamento do sistema, não apenas compilação e testes unitários.

## Instruções

### 1. Determinar a fase

Usar o mesmo método de `/fase-atual` para identificar a fase atual, ou usar o argumento fornecido.

### 2. Ler os Critérios de Conclusão do PRD

Abrir `docs/prd/fase-X-*.md` e localizar a tabela "Critérios de Conclusão".

### 3. Executar cada critério verificável automaticamente

**Para Fase 1:**

```bash
# C1: cargo test --workspace
cargo test --workspace 2>&1 | tail -5

# C2: cargo clippy
cargo clippy --workspace -- -D warnings 2>&1 | grep -c "warning:" || echo "0 warnings"

# C4: Chunk corrompido — buscar o teste de integração
cargo test -p prism-node chunk_security 2>&1 | grep -E "test result|FAILED"

# C5: NodeRecord TTL — buscar o teste de integração
cargo test -p prism-node record_ttl 2>&1 | grep -E "test result|FAILED"

# C6: Rate limiting
cargo test -p prism-node sybil_resistance 2>&1 | grep -E "test result|FAILED"

# C8: Consumo de memória (medição aproximada)
# Iniciar um prism-node e medir RSS após 10s idle
# (requer que o binário exista)
cargo build -p prism-node --release 2>&1 | tail -1
```

**Para Fase 2:**

```bash
# C4: RS(10,4) — reconstrução com 10 de 14
cargo test -p prism-node rs_standard_reconstruct 2>&1

# C5: RS falha com 9 fragmentos
cargo test -p prism-node rs_standard_fails_with_only_9 2>&1

# C6: Chunk com streamer_sig inválida rejeitado
cargo test -p prism-node relay_node_rejects_chunk 2>&1

# C3: Failover — requer ambiente com nós (marcar como manual)
echo "C3 (failover < 1s): REQUER TESTE MANUAL com nós reais"
```

**Para Fase 3:**

```bash
# C2: Ordenação causal
cargo test -p prism-chat concurrent_messages_ordered 2>&1

# C5: Assinatura inválida não propagada
cargo test -p prism-chat invalid_signature_rejected 2>&1

# C6: Blocklist forjada ignorada
cargo test -p prism-chat blocklist_with_invalid_signature 2>&1

# C7: Rate limit
cargo test -p prism-chat rate_limit 2>&1
```

**Para Fase 4:**

```bash
# C1: Ciclo state channel
cargo test -p prism-payments channel_full_cycle 2>&1

# C2: Fraude detectada
cargo test -p prism-payments fraud_detection 2>&1
```

### 4. Identificar critérios que requerem teste manual

Os seguintes critérios dos PRDs **não podem ser verificados automaticamente** e devem ser marcados para validação manual:

- Fase 1 — C6: "Dois nós em redes NAT distintas conectam em < 15s"
- Fase 1 — C7: "prism-node < 50MB RAM com 10 peers"
- Fase 2 — C1: "Viewer recebe stream HLS em Chrome e Safari"
- Fase 2 — C2: "Latência P95 < 20s em rede com 10+ nós"
- Fase 2 — C3: "Failover < 1s quando nó primário cai"
- Fase 2 — C9/C10: "Studio e Player iniciam em X segundos"
- Fase 4 — C5/C6: Cenários de carga e caos com 10.000 viewers / 60% de nós

### 5. Relatório

```
TESTES DE ACEITAÇÃO — PRISM Fase X
====================================

AUTOMÁTICOS:
  C1: cargo test --workspace            [PASS | FAIL]
  C2: cargo clippy -D warnings          [PASS | FAIL — N warnings]
  C3: [nome do critério]                [PASS | FAIL | SKIP — motivo]
  ...

MANUAIS (não executados — requerem ambiente):
  CX: [nome do critério]    → instrução: [como executar manualmente]
  CY: [nome do critério]    → instrução: [como executar manualmente]

CRITÉRIOS AUSENTES (teste definido no PRD mas não encontrado no código):
  CZ: teste "<nome>" não encontrado em crates/

RESULTADO:
  Automáticos: N/M passando
  Manuais pendentes: K critérios
  Ausentes: J testes para escrever

FASE CONCLUÍDA: [Sim (todos automáticos passam, manuais documentados) | Não]
```

### 6. Se um critério falhar

Para cada critério que falhou:
1. Mostrar a saída completa do teste falhando
2. Identificar a causa raiz (qual função / verificação está errada)
3. Indicar qual seção do PRD descreve o comportamento esperado
4. **Não corrigir automaticamente** — reportar e aguardar instrução

Se o argumento incluir `--fix` (ex: `/testar-fase --fix`), corrigir os testes que falharem por implementação incompleta (não por design incorreto).
