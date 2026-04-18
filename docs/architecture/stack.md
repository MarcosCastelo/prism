**Prism**

**Tech Stack Analysis**

Análise Comparativa e Seleção de Tecnologias

_Baseada em benchmarks, estado da arte em 2025-2026 e requisitos do projeto_

Versão 1.0 - Abril 2026

**CONFIDENCIAL - USO INTERNO**

# **1\. Metodologia de Avaliação**

Cada tecnologia foi avaliada contra os requisitos específicos do Prism. As fontes consultadas incluem benchmarks publicados em 2025-2026, documentação oficial, comparativos de produção e evidências de adoção em sistemas similares (streaming P2P, redes distribuídas, clientes desktop de alta performance).

Os critérios de avaliação variam por camada, mas o framework comum considera cinco dimensões:

| **Dimensão**              | **O que avalia**                                                                      |
| ------------------------- | ------------------------------------------------------------------------------------- |
| **Performance**           | Latência, throughput, uso de CPU/memória em condições de carga real                   |
| **Adequação ao problema** | Fit técnico com os requisitos específicos: P2P, baixa latência, real-time, encode AV1 |
| **Maturidade e produção** | Evidências de uso em produção em escala, CVEs conhecidos, frequência de atualizações  |
| **Ecossistema e DX**      | Qualidade da documentação, ferramental, facilidade de integração com outras escolhas  |
| **Risco de longo prazo**  | Tendência de adoção, risco de abandono, dependência de vendor único                   |

_Decisões marcadas como ESCOLHIDO são a recomendação final. ALTERNATIVA indica opções viáveis com trade-offs distintos. DESCARTADO indica tecnologias avaliadas e rejeitadas com justificativa._

# **2\. Linguagem Principal - Core da Rede e Nós**

**Decisão: Rust como linguagem principal do core**

## **2.1 Candidatos Avaliados**

| **Critério**                  | **Rust**                                                 | **Go**                                     | **C++**                                     | **Node.js**                                 |
| ----------------------------- | -------------------------------------------------------- | ------------------------------------------ | ------------------------------------------- | ------------------------------------------- |
| **Performance CPU**           | Excelente - sem GC, zero-cost abstractions               | Muito boa - GC com pausas <10ms            | Excelente - máxima, mas unsafe              | Ruim - GC, single-thread event loop         |
| **Segurança de memória**      | Garantida pelo compilador - sem UB                       | Boa - GC evita maioria dos bugs            | Ruim - UB e buffer overflows comuns         | N/A - gerenciado pelo runtime               |
| **Concorrência P2P**          | Tokio async - excelente para I/O massivo                 | Goroutines - excelente, modelo simples     | Complexo - threads manuais, propenso a bugs | Limitado - não ideal para conexões massivas |
| **rust-libp2p**               | Implementação mais ativa - 5.4k stars, atualizações 2026 | go-libp2p - maduro, usado pelo IPFS        | Sem implementação oficial libp2p            | js-libp2p - menor performance               |
| **Encode AV1 (SVT-AV1)**      | FFI para C direto, sem overhead                          | CGO - funcional mas com overhead de bridge | Nativo - melhor integração teórica          | N-API para C - overhead significativo       |
| **Tamanho de binário**        | ~5-15MB stripped                                         | ~8-20MB com runtime embutido               | ~2-10MB                                     | Requer Node.js runtime - pesado             |
| **DX / Curva de aprendizado** | Alta curva inicial (borrow checker)                      | Baixa curva - simples e direto             | Muito alta - sem guardrails                 | Baixa - mas errado para o domínio           |

## **2.2 Análise e Decisão**

A escolha entre Rust e Go é a mais relevante desta seção. Go foi a linguagem original proposta no documento de arquitetura e merece análise honesta contra Rust.

### **Por que Rust supera Go para o core do Prism**

- rust-libp2p é atualmente a implementação mais ativa do libp2p (5.400+ stars, atualizações em abril/2026), com WebTransport e MoQ em desenvolvimento ativo. go-libp2p é maduro mas sua velocidade de desenvolvimento é menor.
- O core da rede processa centenas de chunks de vídeo simultaneamente por stream ativa. Rust elimina o overhead do GC do Go em operações de alta frequência - crítico para manter latência previsível abaixo de 7s.
- A integração com SVT-AV1 (biblioteca C) é mais limpa em Rust via FFI direto do que Go via CGO, que adiciona overhead de crossing de thread boundary.
- Tauri 2.0 (lançado em outubro/2024, estável em 2025-2026) usa Rust como backend e suporta Windows, macOS, Linux, iOS e Android com um único codebase - isso permite usar o mesmo código do nó para o cliente Studio e Player desktop/mobile.
- Benchmarks publicados em 2025 (Markaicode) mostram Rust consistentemente 2× mais rápido que Go em tarefas CPU-bound como parsing binário e processamento de streams.

### **Onde Go ainda seria superior**

- Velocidade de desenvolvimento inicial - Go produz código em 30-50% menos tempo para desenvolvedores sem experiência em sistemas
- go-libp2p é mais documentado e tem mais exemplos de produção em projetos blockchain/IPFS
- CGO para SVT-AV1 funciona bem na prática mesmo com o overhead teórico

_Decisão: Rust. O Prism processa streams de vídeo em tempo real com requisitos de latência rígidos (7-20s). A ausência de GC pauses, a integração limpa com SVT-AV1 e o ecossistema rust-libp2p ativamente mantido justificam a curva de aprendizado. Tauri 2.0 resolve o problema do cliente com o mesmo runtime, eliminando a necessidade de uma linguagem separada para o Studio._

## **2.3 Estratégia de Linguagens por Componente**

| **Componente**                   | **Linguagem**             | **Justificativa**                                          |
| -------------------------------- | ------------------------- | ---------------------------------------------------------- |
| Core P2P (nó, DHT, relay, RS)    | **Rust**                  | Performance, rust-libp2p, sem GC                           |
| Encoder AV1 (SVT-AV1)            | **C via FFI Rust**        | SVT-AV1 é biblioteca C - FFI direto sem overhead de bridge |
| Studio desktop (streamer client) | **Rust + Tauri 2**        | Reutiliza código do core, <10MB binary, mobile-ready       |
| Player desktop + mobile          | **Rust + Tauri 2**        | iOS/Android suportados nativamente pelo Tauri 2.0          |
| Player web (PWA)                 | **TypeScript + React**    | Browser-native, sem instalação, HLS.js + WebCrypto         |
| Frontend UI (Studio + Player)    | **React + TypeScript**    | Compartilhado entre Tauri e web via Vite                   |
| Protobuf schemas                 | **Proto3 + prost (Rust)** | Geração nativa para Rust, schemas compartilhados           |

# **3\. Protocolo de Transporte entre Nós**

**Decisão: QUIC (quinn) como transporte principal + MoQ como protocolo de mídia**

## **3.1 Contexto - O Emergente Media over QUIC (MoQ)**

Desde a publicação da arquitetura inicial, um desenvolvimento significativo ocorreu na indústria: o protocolo Media over QUIC (MoQ) saiu do IETF Working Group para produção real. Em agosto/2025, a Cloudflare lançou a primeira rede relay MoQ em produção (330+ cidades). Em setembro/2025, nanocosmos tornou-se o primeiro vendor a oferecer MoQ comercialmente.

MoQ é diretamente relevante para o Prism porque resolve exatamente o problema que motivou o design da camada de vídeo: combinar a latência do WebRTC com a escalabilidade do HLS, sobre QUIC, sem os 20 standards complexos do WebRTC.

## **3.2 Comparativo de Protocolos de Transporte**

| **Critério**                      | **TCP/HTTP**                 | **WebRTC/RTP**                  | **QUIC (quinn)**                        | **MoQ over QUIC**                         |
| --------------------------------- | ---------------------------- | ------------------------------- | --------------------------------------- | ----------------------------------------- |
| **Latência de conexão**           | Alta - 3-way handshake + TLS | Alta - ICE+STUN+SDP negotiation | Baixa - 0-RTT possível                  | Baixa - herda 0-RTT do QUIC               |
| **Head-of-line blocking**         | Presente - bloqueia tudo     | Ausente - UDP base              | Ausente - streams independentes         | Ausente - tracks independentes            |
| **NAT traversal**                 | Simples                      | Complexo - ICE/STUN/TURN        | Moderado - QUIC+STUN                    | Moderado - herda do QUIC                  |
| **Escalabilidade 1→N**            | Boa via CDN                  | Ruim - SFU necessário           | Excelente - pub/sub nativo              | Excelente - desenhado para fan-out        |
| **Complexidade da stack**         | Baixa                        | Muito alta - ~20 standards      | Média                                   | Baixa - arquitetura mais limpa            |
| **Maturidade (2026)**             | Máxima                       | Alta - produção há 10+ anos     | Alta - RFC 9000 (2021)                  | Emergente - produção desde 2025           |
| **Suporte browser**               | Total                        | Total                           | Via WebTransport (Chrome/FF)            | Via WebTransport - crescendo              |
| **Melhoria vs WebRTC (latência)** | -                            | Baseline                        | +30% latência, +60% startup (2025 IEEE) | Similar ao QUIC puro + semântica de mídia |

## **3.3 Decisão e Estratégia**

_Decisão: QUIC via quinn (Rust) como transporte principal entre nós. MoQ como protocolo de mídia sobre QUIC para a camada de vídeo - adotado progressivamente conforme a spec estabiliza._

- quinn é a implementação QUIC em Rust mais madura (usada em produção pelo Cloudflare, Mozilla). Integra nativamente com rust-libp2p como transport.
- MoQ endereça diretamente o modelo de pub/subscribe de tracks de vídeo que o Prism usa - a semântica de "named tracks" é exatamente o modelo de layers SVC que projetamos.
- A pesquisa de 2025 (IEEE) confirma 30% de melhoria de latência e 60% de melhoria no tempo de conexão de QUIC vs WebRTC - crucial para manter a stream dentro da janela de 7-20s.
- Estratégia de rollout: QUIC puro na Fase 1-2; MoQ como layer de mídia na Fase 3-4 conforme a spec IETF avança para RFC final.
- WebRTC mantido como fallback para nós que não suportam QUIC (firewalls corporativos que bloqueiam UDP 443) - via TURN relay transparente.

# **4\. Biblioteca P2P e DHT**

**Decisão: rust-libp2p com kad-dht + gossipsub**

## **4.1 Candidatos**

| **Critério**                    | **rust-libp2p**                   | **go-libp2p**       | **Holepunch (hyperswarm)** | **Custom DHT**    |
| ------------------------------- | --------------------------------- | ------------------- | -------------------------- | ----------------- |
| **Kademlia DHT**                | Nativo, maduro                    | Nativo, maduro      | DHT próprio (não Kademlia) | Meses de trabalho |
| **Gossip/PubSub**               | gossipsub nativo                  | gossipsub nativo    | Não nativo                 | A implementar     |
| **QUIC transport**              | quinn integrado                   | Suportado           | UTP (UDP custom)           | A integrar        |
| **WebTransport/MoQ**            | Em desenvolvimento ativo (2026)   | Menor atividade     | Não suportado              | A implementar     |
| **Noise Protocol (encryption)** | Nativo - XX handshake             | Nativo              | Próprio                    | A implementar     |
| **Stars / Atividade (2026)**    | 5.400+ stars, atualiz. abril/2026 | 3.800+ stars, ativo | Ativo, foco P2P apps       | -                 |

_Decisão: rust-libp2p. Melhor alinhamento com o runtime Rust escolhido, desenvolvimento mais ativo, WebTransport/MoQ em roadmap, gossipsub nativo serve como base para o protocolo de chat._

- gossipsub do libp2p substitui a implementação custom de gossip epidemic para o chat - protocolo battle-tested com propriedades de propagação conhecidas e parâmetro de fanout configurável.
- kad-dht do libp2p fornece a DHT Kademlia documentada na arquitetura. Operações FIND_NODE, STORE e FIND_VALUE já implementadas e testadas em produção (IPFS, Ethereum).

# **5\. Encoder AV1 para Streaming ao Vivo**

**Decisão: SVT-AV1 via FFI Rust (libsvtav1)**

## **5.1 Análise dos Encoders AV1**

Três encoders AV1 open-source estão disponíveis. Para streaming ao vivo, o critério mais crítico é encode em tempo real (>= 30fps em 1080p60) - sem isso, o sistema não funciona.

| **Critério**                       | **SVT-AV1 (Intel/Netflix)**                  | **rav1e (Rust, Mozilla)**            | **libaom (Google, referência)**                         |
| ---------------------------------- | -------------------------------------------- | ------------------------------------ | ------------------------------------------------------- |
| **Encode real-time 1080p60**       | Sim - presets 9-12 atingem >60fps            | Marginal - mais lento que SVT-AV1    | Não - 7x mais lento que real-time (benchmark 2022/2025) |
| **Suporte a SVC (Scalable Video)** | Sim - central na arquitetura SVT-AV1         | Limitado - suporte parcial           | Sim - referência, mas lento demais                      |
| **Hardware acceleration**          | Sim - Intel QSV, experimental GPU            | Não nativo                           | Limitado                                                |
| **Qualidade de compressão**        | Muito boa - ~3% abaixo de libaom em VMAF     | Boa - entre SVT-AV1 e libaom         | Melhor - encoder de referência                          |
| **Modo low-delay**                 | Sim - projetado para videoconferência e live | Suporte básico                       | Não recomendado para real-time                          |
| **Integração Rust FFI**            | svt-av1 crate disponível no crates.io        | rav1e é escrito em Rust - API nativa | aom-sys crate via FFI                                   |

_Decisão: SVT-AV1. É o único encoder AV1 open-source capaz de encode em tempo real de 1080p60 com suporte pleno a SVC. A diferença de qualidade vs libaom (~3% em VMAF) é irrelevante frente à impossibilidade de libaom rodar em real-time. rav1e é alternativa para hardware muito fraco como fallback de baixa qualidade._

# **6\. Framework Desktop e Mobile - Studio e Player**

**Decisão: Tauri 2.x (substituindo Wails)**

## **6.1 Por que Tauri 2 em vez de Wails**

O documento de arquitetura original escolheu Wails (Go + WebView). Com a decisão de migrar o core para Rust, e com o lançamento estável do Tauri 2.0 em outubro/2024 (com suporte a iOS e Android), Tauri 2 torna-se a escolha claramente superior.

| **Critério**                   | **Tauri 2**                 | **Wails v3**              | **Electron**                      | **Flutter**                    |
| ------------------------------ | --------------------------- | ------------------------- | --------------------------------- | ------------------------------ |
| **Mobile (iOS/Android)**       | Sim - Tauri 2.0 estável     | Não - desktop only        | Não                               | Sim                            |
| **Backend Rust**               | Sim - nativo                | Não - Go apenas           | Node.js                           | Dart                           |
| **Reuso de código do core**    | Direto - mesmo runtime Rust | Não - Go separado do Rust | Não                               | Não                            |
| **Tamanho do binário**         | <10MB                       | <15MB                     | \>100MB (inclui Chromium)         | ~20-40MB                       |
| **Memória idle**               | 30-40MB                     | ~50MB                     | 200-300MB                         | ~80MB                          |
| **Tempo de startup**           | <0.5s                       | ~0.5s                     | 1-2s                              | <0.5s                          |
| **Frontend compartilhado web** | Sim - mesmo React/TS da PWA | Sim - mesmo React/TS      | Sim                               | Não - Dart/Flutter UI separada |
| **Stars / Tendência (2026)**   | 105k stars, +55% YoY        | 33k stars, crescendo      | 121k stars, crescimento estagnado | 176k stars, forte              |

_Decisão: Tauri 2. Mobile nativo (iOS + Android) a partir do mesmo codebase Rust, binários <10MB, memória idle 30-40MB vs 200-300MB do Electron, e reuso direto das crates do core P2P. Hoppscotch (produção) migrou Electron → Tauri e reportou 165MB → 8MB e 70% de redução de memória._

# **7\. Protocolo de Chat - Gossip e Ordenação**

**Decisão: libp2p gossipsub + vector clocks custom em Rust**

## **7.1 Alternativas para Disseminação de Mensagens**

| **Critério**                         | **libp2p gossipsub**                                    | **Implementação custom de gossip** | **Matrix protocol**                 |
| ------------------------------------ | ------------------------------------------------------- | ---------------------------------- | ----------------------------------- |
| **Já disponível no stack**           | Sim - parte do rust-libp2p                              | Não - trabalho de meses            | Não - protocolo externo             |
| **Fanout e cobertura configuráveis** | Sim - parâmetros mesh exatos                            | Sim                                | Não adequado para real-time         |
| **Latência de propagação**           | O(log N) rounds - adequado para <500ms                  | Similar                            | Maior - HTTP polling baseado        |
| **Integração com DHT/P2P**           | Nativa - mesma Swarm do libp2p                          | Requer integração manual           | Protocolo separado                  |
| **Ordenação causal**                 | Não incluso - implementar vector clocks sobre gossipsub | A implementar                      | Ordering próprio, diferente do spec |

_Decisão: libp2p gossipsub para transporte de mensagens (já no stack, sem código extra) + implementação custom de vector clocks e prev-hash chain em Rust para ordenação causal. Separação clara de responsabilidades: gossipsub propaga, vector clocks ordenam._

# **8\. Sincronização Temporal entre Nós**

**Decisão: PTP via timestamping QUIC + fallback NTP disciplinado**

## **8.1 Alternativas**

| **Critério**                      | **PTP (IEEE 1588)**                           | **NTP**                       | **Timestamps QUIC relativos**     |
| --------------------------------- | --------------------------------------------- | ----------------------------- | --------------------------------- |
| **Precisão**                      | <1ms - sub-milissegundo                       | 10-50ms de erro típico        | N/A - relativo, não absoluto      |
| **Requer hardware especial**      | PTP hardware: não. Software PTP: sim mas <1ms | Não                           | Não                               |
| **Complexidade de implementação** | Alta - daemon ptpd ou chrony+PTP              | Baixa - chrony ou ntpd padrão | Baixa - nativo no QUIC            |
| **Funciona em internet pública**  | Sim - PTP over UDP                            | Sim                           | Sim - mas sem referência absoluta |

Para o Prism, o playout buffer de 7-20s tolera erros de sincronização de até ~500ms sem impacto perceptível. NTP disciplinado com chrony oferece precisão de ~10-50ms - suficiente para o caso de uso, com implementação trivial.

_Decisão: NTP disciplinado (chrony) como requisito de instalação do nó, com playout buffer dimensionado para absorver a imprecisão residual. PTP é mantido como opção avançada para operadores que querem reduzir drift para <1ms. Timestamps QUIC são usados para medir RTT inter-nós e calcular health scores dinamicamente._

# **9\. Erasure Coding - Reed-Solomon**

**Decisão: reed-solomon crate (Rust) - implementação Backblaze**

## **9.1 Análise**

| **Critério**                     | **reed-solomon (crates.io)**                                   | **raptorq (fountain codes)**                                        |
| -------------------------------- | -------------------------------------------------------------- | ------------------------------------------------------------------- |
| **Performance**                  | Muito boa - implementação SIMD-optimized (Backblaze algorithm) | Excelente para grandes volumes, overhead maior para chunks pequenos |
| **Parâmetros n,k configuráveis** | Sim - RS(10,4) e RS(4,2) suportados                            | Sim                                                                 |
| **Adequação para chunks de 3s**  | Ideal - latência de encode/decode <1ms para chunks típicos     | Fountain codes têm vantagem apenas com loss >30% - atípico aqui     |
| **Crate Rust nativa**            | Sim - sem FFI                                                  | Sim                                                                 |

_Decisão: reed-solomon crate. Reed-Solomon com parâmetros configuráveis é a escolha padrão da indústria para este caso de uso. RaptorQ teria vantagem apenas em condições de loss muito alto (>30%), atípicas para a rede descentralizada do Prism._

# **10\. Player Web - HLS e Adaptive Bitrate**

**Decisão: HLS.js + hls-parser + Media Source Extensions**

## **10.1 Análise de Players HLS para Browser**

| **Critério**                 | **HLS.js**                                              | **Video.js + HLS plugin** | **Shaka Player**           |
| ---------------------------- | ------------------------------------------------------- | ------------------------- | -------------------------- |
| **ABR automático**           | Excelente - algoritmo EWMA maduro                       | Bom                       | Excelente - BOLA algorithm |
| **Suporte a AV1**            | Sim - via fMP4 container                                | Depende do plugin         | Sim                        |
| **TypeScript nativo**        | Sim - tipos completos                                   | Parcial                   | Sim                        |
| **Tamanho bundle (gzipped)** | ~45KB                                                   | ~200KB+ com plugins       | ~180KB                     |
| **Adoção e manutenção**      | Padrão de mercado - usado por Twitch, Twitter, LinkedIn | Ampla adoção              | Google - muito bem mantido |

_Decisão: HLS.js. Bundle menor, TypeScript nativo, algoritmo ABR maduro, e é a biblioteca usada pela própria Twitch em produção. Shaka Player é alternativa válida mas o overhead de bundle (~4x HLS.js) não se justifica para o caso de uso._

# **11\. Criptografia e Identidade**

**Decisão: Ed25519 via dalek-cryptography (Rust) + WebCrypto API (browser)**

## **11.1 Análise de Algoritmos de Assinatura**

| **Critério**                    | **Ed25519**                            | **ECDSA (P-256)**          | **RSA-2048**        | **BLS12-381**            |
| ------------------------------- | -------------------------------------- | -------------------------- | ------------------- | ------------------------ |
| **Tamanho da chave pública**    | 32 bytes                               | 64 bytes                   | 256 bytes           | 48 bytes                 |
| **Tamanho da assinatura**       | 64 bytes                               | 64 bytes                   | 256 bytes           | 96 bytes                 |
| **Performance de verificação**  | Muito rápida                           | Rápida                     | Lenta               | Lenta                    |
| **WebCrypto API (browser)**     | Sim - Chrome 113+, FF 119+, Safari 17+ | Sim - amplo suporte        | Sim - amplo suporte | Não - sem suporte nativo |
| **Resistente a timing attacks** | Sim - por design                       | Necessita cuidado especial | Necessita cuidado   | Sim                      |
| **Suporte em rust-libp2p**      | Nativo - PeerId derivado de Ed25519    | Suportado                  | Legado              | Não padrão               |

_Decisão: Ed25519. Chaves de 32 bytes se encaixam perfeitamente como identidade P2P (PeerId do libp2p já usa Ed25519 nativamente). Suporte WebCrypto em todos os browsers modernos elimina a necessidade de fallback WASM para a grande maioria dos usuários. Crate ed25519-dalek em Rust é auditada e amplamente usada em produção._

# **12\. Serialização de Dados em Rede**

**Decisão: Protocol Buffers v3 via prost (Rust)**

| **Critério**                | **Protobuf/prost**                               | **MessagePack**                       | **CBOR**        | **JSON**                        |
| --------------------------- | ------------------------------------------------ | ------------------------------------- | --------------- | ------------------------------- |
| **Tamanho encoded**         | Muito compacto - sem field names                 | Compacto                              | Compacto        | Verbose - field names repetidos |
| **Schema e versionamento**  | Schema obrigatório - breaking changes detectados | Sem schema - sem detecção de breaking | Schema opcional | Sem schema formal               |
| **Performance decode Rust** | Muito rápido - zero-copy com prost-bytes         | Rápido                                | Bom             | Lento para payloads grandes     |
| **Geração de código Rust**  | prost - geração automática via build.rs          | Não - implementação manual            | Não padrão      | serde_json - manual             |

_Decisão: Protobuf v3 com prost. Schema versionado evita bugs silenciosos de protocolo ao longo do desenvolvimento. Payload compacto é crítico para chunks de 3s de vídeo sendo retransmitidos entre nós com frequência alta. prost gera código Rust idiomático a partir dos .proto files._

# **13\. Stack Final Consolidado**

**Resumo executivo - todas as decisões em uma tabela**

| **Camada**                   | **Tecnologia escolhida**                    | **Alternativa descartada** | **Razão principal da escolha**                                                              |
| ---------------------------- | ------------------------------------------- | -------------------------- | ------------------------------------------------------------------------------------------- |
| **Linguagem core**           | **Rust + Tokio async**                      | Go                         | Sem GC, FFI limpo com SVT-AV1, rust-libp2p mais ativo, Tauri 2 reutiliza runtime            |
| **Transporte entre nós**     | **QUIC (quinn) + MoQ (fase 3+)**            | WebRTC/RTP                 | +30% latência, +60% startup vs WebRTC; MoQ resolve pub/sub de mídia nativamente             |
| **P2P / DHT**                | **rust-libp2p (kad-dht + gossipsub)**       | go-libp2p / custom         | Mais ativo em 2026, WebTransport/MoQ em roadmap, gossipsub resolve o chat                   |
| **Encoder AV1**              | **SVT-AV1 via FFI Rust**                    | libaom (ref. impl)         | Único encoder capaz de real-time 1080p60 com SVC; libaom 7x mais lento que real-time        |
| **Cliente desktop + mobile** | **Tauri 2.x (Rust + React/TS)**             | Wails (Go) / Electron      | iOS/Android nativos, <10MB binário, 30-40MB RAM, reusa crates do core Rust                  |
| **Player web (PWA)**         | **React + HLS.js + WebRTC**                 | Video.js / Shaka           | Bundle 4x menor que alternativas, TypeScript nativo, padrão de mercado (Twitch)             |
| **UI framework**             | **React + TypeScript + Vite**               | Svelte / Vue               | Compartilhado entre Tauri e PWA sem reescrita; ecossistema dominante                        |
| **Chat - transporte**        | **libp2p gossipsub**                        | Custom gossip              | Já no stack, sem código extra, propriedades conhecidas                                      |
| **Chat - ordenação causal**  | **Vector Clocks + prev-hash (Rust custom)** | Matrix protocol            | Leve, sem dependência externa, implementação bem documentada                                |
| **Sincronização temporal**   | **NTP (chrony) + playout buffer**           | PTP IEEE 1588              | Precisão de 10-50ms suficiente para buffer de 7-20s; implementação trivial                  |
| **Erasure coding**           | **reed-solomon crate (Rust)**               | RaptorQ                    | RS suficiente para loss < 30%; encode/decode <1ms por chunk; sem overhead de fountain codes |
| **Serialização**             | **Protocol Buffers v3 + prost**             | MessagePack / JSON         | Schema versionado, payload compacto, geração de código Rust automática                      |
| **Identidade / Assinatura**  | **Ed25519 (ed25519-dalek / WebCrypto)**     | ECDSA P-256                | Nativo no libp2p PeerId, chaves 32 bytes, suporte WebCrypto em 2026                         |
| **NAT traversal**            | **ICE/STUN + TURN fallback (coturn)**       | -                          | QUIC resolve a maior parte; TURN apenas como último recurso para CGNAT                      |
| **Build system**             | **Cargo (Rust) + Vite (frontend)**          | Make / Bazel               | Ferramental nativo Rust, sem configuração extra                                             |
| **CI/CD**                    | **GitHub Actions + tauri-action**           | GitLab CI / Jenkins        | tauri-action automatiza builds multiplataforma incluindo iOS/Android                        |

# **14\. Delta - Mudanças em Relação ao Documento de Arquitetura**

Este documento identificou quatro mudanças significativas em relação às tecnologias propostas originalmente no documento de arquitetura:

| **Área**               | **Original**              | **Revisado**          | **Impacto**                                                                  |
| ---------------------- | ------------------------- | --------------------- | ---------------------------------------------------------------------------- |
| **Linguagem do core**  | Go                        | Rust                  | Alto - reescrita do core, ganho de performance e unificação com Tauri        |
| **Framework desktop**  | Wails (Go)                | Tauri 2 (Rust)        | Alto - mobile (iOS/Android) passa a ser nativo; mesmo runtime do core        |
| **Protocolo de mídia** | HLS P2P + WebRTC SFU      | QUIC + MoQ (fase 3-4) | Médio - MoQ não existia como protocolo produtivo em 2024; adoção incremental |
| **Gossip do chat**     | Custom epidemic broadcast | libp2p gossipsub      | Baixo - comportamento idêntico, menos código para manter                     |

_Prism Tech Stack Analysis v1.0 - Documento vivo. Revisão recomendada a cada fase do roadmap._