**Prism**

Plataforma de Streaming Descentralizado ao Vivo

_Documento de Arquitetura, PRD e Plano de Implementação_

Versão 1.0 - Abril 2026

**CONFIDENCIAL - USO INTERNO**

# **Índice**

# **1\. Visão Geral do Projeto**

## **1.1 Contexto e Motivação**

Prism é uma plataforma de streaming de vídeo ao vivo 100% descentralizada, sem nenhum ponto central de controle, falha ou censura. A plataforma elimina intermediários, distribui computação por toda a rede de participantes e garante que qualquer pessoa possa transmitir e assistir com qualidade comparável às plataformas centralizadas.

As plataformas centralizadas existentes apresentam limitações estruturais que só podem ser resolvidas com uma abordagem fundamentalmente diferente:

- Ponto único de falha - uma interrupção derruba toda a plataforma
- Censura unilateral - streamer pode ser removido sem recurso
- Monetização exploratória - plataforma retém 50% ou mais da receita
- Rastreamento de audiência - todo viewer é monitorado e monetizado
- Custo de infraestrutura concentrado - dependência de AWS/GCP/Azure

## **1.2 Princípios de Design**

- Zero servidores centrais - nenhuma entidade controla a rede
- Degradação graciosa - a stream nunca para, apenas reduz qualidade se a rede encolhe
- Proporcionalidade computacional - cada nó contribui conforme sua capacidade
- Verificabilidade criptográfica - toda operação é auditável sem confiança implícita
- Separação de camadas - vídeo e chat operam em stacks independentes mas coordenadas

## **1.3 Restrições de Design Definidas**

| **Restrição**                 | **Valor / Justificativa**                             |
| ----------------------------- | ----------------------------------------------------- |
| **Latência máxima**           | 7 - 20 segundos streamer → viewer                     |
| **Latência do chat**          | < 500 ms (tempo real absoluto, independente do vídeo) |
| **Servidores centrais**       | Zero - toda coordenação via DHT/P2P                   |
| **Transcoding centralizado**  | Zero - AV1/SVC elimina reencoding na rede             |
| **Protocolo de vídeo**        | HLS sobre P2P com segmentos de 3 segundos             |
| **Mínimo de nós para operar** | 1 nó seed + streamer (degrada mas funciona)           |

# **2\. Arquitetura do Sistema**

## **2.1 Visão de Camadas**

O sistema é dividido em três camadas independentes que compartilham apenas a camada de identidade criptográfica e a DHT de descoberta de peers.

| **Camada**                | **Responsabilidade**           | **Tecnologia Central**         |
| ------------------------- | ------------------------------ | ------------------------------ |
| **Identidade**            | Autenticação sem servidor      | Ed25519, chaves locais         |
| **Descoberta**            | Localizar peers e nós da rede  | DHT Kademlia                   |
| **Vídeo**                 | Ingest, processamento, entrega | AV1/SVC, HLS P2P, Reed-Solomon |
| **Chat**                  | Mensagens em tempo real        | Gossip, Vector Clocks, Ed25519 |
| **Coordenação econômica** | Incentivo a nós, pagamentos    | State Channels off-chain       |

## **2.2 Camada de Identidade**

Cada participante da rede - streamer, nó relay, viewer - possui um par de chaves Ed25519 gerado localmente no primeiro acesso. A chave pública é a identidade permanente. A chave privada jamais sai do dispositivo.

### **Geração e uso das chaves**

- Par Ed25519 gerado via CSPRNG no dispositivo do usuário
- Chave pública = identificador único da identidade (equivale a username)
- Toda mensagem de chat, todo chunk de vídeo e toda entrada na DHT é assinada com a chave privada do emissor
- Qualquer nó verifica assinaturas sem consultar servidor externo
- Display name é um texto livre assinado pela chave - dois usuários podem ter o mesmo nome, nunca a mesma chave

## **2.3 Camada de Descoberta - DHT Kademlia**

A DHT (Distributed Hash Table) baseada em Kademlia é o único mecanismo de coordenação global. Não existe servidor de bootstrap permanente - os primeiros nós são descobertos via lista de peers conhecidos embutida no cliente (similar ao Bitcoin).

### **Estrutura do espaço de endereçamento**

node_id = SHA-256(public_key) → 256 bits

stream_id = SHA-256(streamer_pubkey + timestamp_inicio) → 256 bits

Distância entre nós = XOR(node_id_A, node_id_B)

### **Operações fundamentais**

- FIND_NODE(target_id) - localiza os K nós mais próximos de um ID em O(log N) saltos
- STORE(key, value) - armazena um registro (ex: metadados de stream) nos K nós responsáveis
- FIND_VALUE(key) - recupera um registro armazenado

### **Registros armazenados na DHT**

| **Chave**             | **Valor**                        | **TTL**                           |
| --------------------- | -------------------------------- | --------------------------------- |
| stream_id             | metadados + nós seed ativos      | Enquanto stream ativa + 60s       |
| node_id               | capacidade, region, health_score | 10 segundos (renovado ativamente) |
| stream_id + "anchors" | lista de anchor nodes do chat    | Enquanto stream ativa             |

## **2.4 Camada de Vídeo - Pipeline Completo**

### **2.4.1 Responsabilidade do Host (Streamer)**

O host executa apenas operações que dependem exclusivamente do acesso à fonte bruta de áudio/vídeo. Após injetar um chunk na rede, o host não tem mais responsabilidade sobre aquele chunk.

- Captura A/V via dispositivo local (OBS, cliente nativo, browser via MediaDevices API)
- Encode AV1 com SVC (Scalable Video Coding) - uma única passagem gera todas as camadas de qualidade
- Segmentação em chunks de 3 segundos
- Assinatura Ed25519 de cada chunk - garante autenticidade em toda a rede
- Injeção simultânea em 4 nós seed descobertos via DHT (redundância inicial)
- Carga de banda do host: bitrate_original × 4 - independente do número de viewers

_⚠ O host NÃO mantém conexões com viewers, NÃO rastreia a topologia, NÃO faz transcoding de resoluções adicionais. Tudo isso é responsabilidade da rede._

### **2.4.2 Encode AV1/SVC - Estrutura de Layers**

O codec AV1 com Scalable Video Coding permite que um único stream codificado contenha múltiplas qualidades embutidas como camadas independentes. Nós intermediários nunca precisam decodificar ou re-encodar.

| **Layer**          | **Resolução** | **Framerate** | **Bitrate** | **Tipo**    |
| ------------------ | ------------- | ------------- | ----------- | ----------- |
| Base (L0)          | 360p          | 30fps         | 400 kbps    | Obrigatória |
| Enhancement 1 (L1) | 480p          | 30fps         | +600 kbps   | Opcional    |
| Enhancement 2 (L2) | 720p          | 60fps         | +1.5 Mbps   | Opcional    |
| Enhancement 3 (L3) | 1080p         | 60fps         | +3 Mbps     | Opcional    |

Layer stripping é uma operação bitwise - remover bytes do pacote RTP sem decodificar nada. Custo de CPU no nó que faz o strip: desprezível.

### **2.4.3 Classes de Nós e Distribuição de Trabalho**

Cada nó anuncia sua capacidade disponível na DHT a cada 10 segundos. O sistema atribui tarefas proporcionalmente, sem coordenação central.

| **Classe** | **Perfil típico**                    | **Tarefas atribuídas**                                                                                      | **Carga estimada**              |
| ---------- | ------------------------------------ | ----------------------------------------------------------------------------------------------------------- | ------------------------------- |
| **A**      | Nó com pouca CPU, boa banda          | Remuxing de container, geração de manifests HLS/DASH, injeção de timestamps PTP                             | ~5% CPU, I/O dominante          |
| **B**      | Nó intermediário com GPU ou CPU boa  | Transcoding da layer base L0 para H.264 (compatibilidade), geração de thumbnails, fragmentação Reed-Solomon | ~15-30% CPU/GPU                 |
| **C**      | Nó com armazenamento disponível      | Armazenamento de fragmentos Reed-Solomon, resposta a late joiners, cache de chunks recentes                 | ~5% CPU, storage intensivo      |
| **Edge**   | Nó próximo geograficamente ao viewer | Layer stripping final, entrega HLS ao viewer, medição de qualidade de conexão                               | ~5-15% CPU, conexões intensivas |

### **2.4.4 Consistent Hashing com Pesos Dinâmicos**

A distribuição de streams por nós usa Consistent Hashing com vnodes ponderados. Nós mais capazes ocupam mais posições no anel de hash, recebendo proporcionalmente mais trabalho.

weight = min(cpu_available%, bw_available%) / normalization_factor

vnodes_count = floor(weight × MAX_VNODES)

\# Anúncio na DHT a cada 10s se |new_weight - current_weight| > threshold

Quando um nó reduz seu peso (por aumento de carga), novas streams são automaticamente roteadas para outros nós. Não há reconfiguração global - apenas os vizinhos na DHT são atualizados.

### **2.4.5 Redundância com Erasure Coding (Reed-Solomon)**

Em vez de replicar chunks inteiros (custo N×100%), a rede usa Reed-Solomon para fragmentar chunks com overhead controlado.

Parâmetros padrão (muitos nós): RS(10, 4)

→ 10 fragmentos de dados + 4 fragmentos de paridade = 14 fragmentos totais

→ quaisquer 10 dos 14 fragmentos reconstroem o chunk original

→ tolera perda de até 4 fragmentos simultâneos

→ overhead: 40% de armazenamento adicional

Parâmetros reduzidos (poucos nós): RS(4, 2)

→ 4 fragmentos de dados + 2 fragmentos de paridade = 6 fragmentos totais

→ quaisquer 4 dos 6 fragmentos reconstroem o chunk original

→ tolera 2 nós caindo simultaneamente

→ overhead: 50%

Os fragmentos são distribuídos entre nós de Classe C geograficamente distintos. A reconstrução é feita pelo nó edge ou pelo viewer diretamente.

### **2.4.6 Topologia da Rede de Vídeo**

A rede usa topologia de árvore com fanout máximo fixo de 8 filhos por nó. Cada nó mantém um pai primário e um pai backup com conexão já estabelecida para failover instantâneo.

Árvore com fanout=8:

\[Streamer/Host\]

│

\[Nó Raiz/Seed\] fanout=8

/ / | \\ \\

\[A\] \[B\] \[C\] \[D\] \[E\] fanout=8 cada

/\\ /\\ ...

\[F\]\[G\]\[H\]\[I\]

Cada nó mantém:

pai_primario: conexão ativa, recebe stream

pai_backup: conexão QUIC aberta, stream pausada → failover em ~0ms

Quando um nó cai, seus filhos ativam imediatamente a conexão com o pai backup. A conexão WebRTC já estava estabelecida - não há handshake adicional no momento da falha.

### **2.4.7 Sincronização Temporal - PTP + Playout Buffer**

Nós em diferentes pontos da árvore recebem chunks em momentos diferentes, criando drift temporal entre viewers. O mecanismo de sincronização garante que todos os viewers vejam o mesmo frame no mesmo instante absoluto dentro da janela de 7-20 segundos.

deadline_global = timestamp_origem + latência_máxima_configurada

Nó A (recebeu em t+200ms): espera até deadline_global - 200ms

Nó D (recebeu em t+800ms): espera até deadline_global - 800ms

→ todos entregam no mesmo instante

A sincronização de relógio entre nós usa NTP disciplinado (chrony) como requisito de instalação do nó, com precisão de 10–50ms — suficiente para o playout buffer de 7–20s. PTP (Precision Time Protocol / IEEE 1588) é mantido como opção avançada para operadores que precisam de drift < 1ms.

- Nós com jitter > threshold anunciam health_score baixo na DHT
- Nós com health_score baixo são evitados pelo algoritmo de roteamento
- Viewers são automaticamente redirecionados para nós alternativos

### **2.4.8 Recuperação de Pacotes Perdidos - NACK Seletivo**

Nó B recebe seq: 1001, 1002, 1004 → gap detectado em 1003

→ envia NACK para Nó A pedindo retransmissão de 1003

→ se Nó A não tem: propaga NACK upstream

→ timeout: se não chega antes do deadline → descarta

player usa interpolação/frame concealment

O timeout de NACK é calculado como: deadline_global - tempo_atual - margem_de_segurança. Tentar recuperar após o deadline piora a experiência - é melhor entregar um frame interpolado do que atrasar todos os frames seguintes.

## **2.5 Camada de Chat**

O chat opera em tempo real absoluto (wall clock), completamente independente do buffer de vídeo de cada viewer. Um viewer com 20 segundos de buffer vê o chat do presente, não do passado - comportamento idêntico às plataformas centralizadas.

### **2.5.1 Protocolo de Transporte - Gossip Epidemic**

Mensagens são propagadas por broadcast epidêmico (gossip). Cada nó que recebe uma mensagem a repassa para 3 peers aleatórios do seu routing table da DHT.

Cobertura da rede: 1 - (1/fanout)^rounds

Com fanout=3, rounds=log(N):

N=100 nós → ~5 rounds → ~250ms para cobertura total

N=1000 nós → ~7 rounds → ~350ms para cobertura total

N=10000 nós → ~10 rounds → ~500ms para cobertura total

Duplicatas são descartadas por hash da mensagem. Cada nó mantém um cache LRU dos últimos 10.000 hashes de mensagens já vistas, com TTL de 5 minutos.

### **2.5.2 Estrutura de Payload de Mensagem**

{

"id": "SHA256(sender_pubkey + text + timestamp)",

"sender": "ed25519_pubkey_hex",

"text": "conteúdo da mensagem",

"timestamp": 1713196800412, // Unix ms - wall clock apenas

"prev": "id_da_ultima_msg_que_o_sender_viu",

"sig": "ed25519_signature_hex"

}

Não existe campo stream_clock ou qualquer sincronização com o vídeo. O timestamp é exclusivamente o tempo real de envio.

### **2.5.3 Ordenação Causal - Vector Clocks + Prev-Hash Chain**

Vector Clocks garantem que mensagens concorrentes sejam ordenadas deterministicamente por todos os nós, sem coordenação central.

- Cada participante mantém um vetor de contadores {pubkey: count} para todos os participantes conhecidos
- Mensagens com prev desconhecido são seguradas em buffer até o prev chegar
- Mensagens concorrentes (enviadas sem conhecimento mútuo) são ordenadas por hash da mensagem - determinístico e sem comunicação adicional
- O buffer de reordenação por nó tem TTL de 30 segundos - mensagens não resolvidas nesse tempo são descartadas com marcador de gap visível no chat

### **2.5.4 Histórico para Late Joiners - Anchor Nodes**

Nós voluntários com uptime alto e armazenamento disponível se candidatam como anchors. Múltiplos anchors existem simultaneamente para redundância.

- Anchors se anunciam na DHT sob a chave stream_id + "anchors"
- Armazenam as últimas 500 mensagens com seus metadados completos
- Novo viewer consulta a DHT, localiza os anchors, solicita histórico + vector clock atual
- O histórico é verificável - cada mensagem carrega assinatura Ed25519 do remetente original
- Após receber o histórico, o viewer começa a participar do gossip a partir do estado atual

### **2.5.5 Moderação Distribuída**

Sem servidor central, a moderação é aplicada localmente por cada cliente baseado em listas assinadas pelo streamer.

- Streamer publica blocklist/allowlist assinada com sua chave Ed25519 - propagada via gossip
- Cada viewer aplica a lista localmente ao renderizar o chat
- Viewers que não respeitam a lista veem as mensagens filtradas - problema social, não técnico
- Score de reputação local por pubkey: comportamento de spam reduz score, histórico longo aumenta
- Scores são compartilhados via gossip - reputação ruim se propaga pela rede sem coordenação central

## **2.6 Fluxo Completo de Dados**

┌──────────────────────────────────────────────────────────────────┐

│ HOST/STREAMER │

│ Captura A/V → Encode AV1/SVC → Segmenta 3s → Assina Ed25519 │

│ → Injeta em 4 nós seed via DHT │

└──────────────────────────┬───────────────────────────────────────┘

│ chunk assinado (todas as layers)

▼

┌──────────────────────────────────────────────────────────────────┐

│ CAMADA 1 - NÓS SEED (Classe A) │

│ Verifica assinatura → Remux para HLS/DASH → Gera manifests │

│ → Fragmenta Reed-Solomon → Distribui para Camada 2 │

└──────────────────────────┬───────────────────────────────────────┘

│

▼

┌──────────────────────────────────────────────────────────────────┐

│ CAMADA 2 - NÓS INTERMEDIÁRIOS (Classe B) │

│ Transcoding L0→H.264 (compatibilidade) → Thumbnails │

│ → Armazena fragmento RS → Distribui para Camada 3/Edge │

└──────────────────────────┬───────────────────────────────────────┘

│

▼

┌──────────────────────────────────────────────────────────────────┐

│ NÓS EDGE │

│ Layer stripping conforme capacidade do viewer │

│ → Entrega HLS ao viewer via HTTP │

└──────────────────────────┬───────────────────────────────────────┘

│

▼

┌──────────────────────────────────────────────────────────────────┐

│ VIEWER │

│ Player HLS → ABR automático → Decode AV1 │

│ Latência total: 7-20s │

└──────────────────────────────────────────────────────────────────┘

CHAT (camada paralela, independente):

Viewer/Streamer → assina msg Ed25519 → gossip para 3 peers

→ cobertura da rede em O(log N) rounds → ~200-500ms

# **3\. Product Requirements Document (PRD)**

## **3.1 Objetivo do Produto**

Prism entrega uma experiência de streaming ao vivo onde o streamer instala um cliente, configura sua chave e começa a transmitir - sem criar conta em servidor central, sem aprovação de plataforma e sem depender de infraestrutura de terceiros para a stream continuar funcionando.

## **3.2 Usuários e Casos de Uso Primários**

| **Usuário**        | **Objetivo principal**              | **Caso de uso chave**                                  |
| ------------------ | ----------------------------------- | ------------------------------------------------------ |
| **Streamer**       | Transmitir sem intermediário        | Iniciar live, ver viewers, receber pagamentos diretos  |
| **Viewer**         | Assistir com qualidade adaptativa   | Descobrir streams, assistir, participar do chat        |
| **Operador de nó** | Contribuir infra e ser recompensado | Instalar nó, configurar capacidade, monitorar earnings |

## **3.3 Requisitos Funcionais**

### **RF-01: Streaming de Vídeo**

- RF-01.1: Streamer deve poder iniciar uma live em menos de 30 segundos após abrir o cliente
- RF-01.2: A stream deve estar disponível para viewers em até 20 segundos do início da transmissão
- RF-01.3: O sistema deve suportar qualidade adaptativa automática (360p a 1080p60) baseada na conexão do viewer
- RF-01.4: A stream não deve interromper quando nós individuais caem (failover via pai backup em <1 segundo)
- RF-01.5: A stream deve funcionar com apenas 1 nó seed ativo (qualidade degradada mas funcional)

### **RF-02: Chat em Tempo Real**

- RF-02.1: Mensagens de chat devem ser entregues a todos os viewers em menos de 500ms em condições normais de rede
- RF-02.2: Viewers que entram no meio da live devem receber as últimas 500 mensagens de histórico
- RF-02.3: Mensagens devem ser exibidas em ordem causal - nunca uma resposta antes da pergunta
- RF-02.4: O streamer deve poder bloquear participantes via blocklist assinada propagada pela rede
- RF-02.5: O chat deve operar em tempo real absoluto, independente do buffer de vídeo de cada viewer

### **RF-03: Identidade e Autenticidade**

- RF-03.1: Identidade criada localmente - sem cadastro em servidor
- RF-03.2: Toda mensagem de chat deve ser verificável criptograficamente pelo receptor
- RF-03.3: Cada chunk de vídeo deve carregar assinatura do streamer - viewers devem poder verificar autenticidade
- RF-03.4: Display name é configurável livremente - chave pública é o identificador único

### **RF-04: Descoberta de Streams**

- RF-04.1: Viewer deve conseguir encontrar uma stream ativa pela chave pública do streamer
- RF-04.2: Lista de streams ativas deve ser consultável via DHT sem servidor de índice central
- RF-04.3: Streamer pode optar por stream pública (anunciada na DHT) ou privada (compartilhada por link/chave)

### **RF-05: Operação de Nó**

- RF-05.1: Instalação do nó deve ser possível via linha de comando em menos de 5 minutos
- RF-05.2: Nó deve auto-configurar sua classe (A/B/C) baseado em benchmark automático no startup
- RF-05.3: Nó deve anunciar e atualizar capacidade disponível na DHT a cada 10 segundos
- RF-05.4: Nó deve operar corretamente em NAT sem configuração manual (ICE/STUN/TURN)

## **3.4 Requisitos Não-Funcionais**

| **ID** | **Categoria**     | **Requisito**                            | **Métrica de aceitação**                      |
| ------ | ----------------- | ---------------------------------------- | --------------------------------------------- |
| RNF-01 | Latência de vídeo | 7-20s streamer→viewer                    | P95 < 20s em rede com 10+ nós                 |
| RNF-02 | Latência de chat  | < 500ms                                  | P95 < 500ms em rede com 10+ nós               |
| RNF-03 | Disponibilidade   | Stream não para com falha de nós         | Tolerância a 60% dos nós caindo (dual-parent failover + DHT rerouting + RS 10,4)     |
| RNF-04 | Escalabilidade    | Performance não degrada com mais viewers | Latência estável de 10 para 10.000 viewers    |
| RNF-05 | Segurança         | Chunks inválidos rejeitados na borda     | 100% chunks sem assinatura válida descartados |
| RNF-06 | Compatibilidade   | Suporte a players HLS padrão             | Funciona em Chrome, Firefox, Safari, VLC      |

# **4\. Stack Tecnológico**

## **4.1 Vídeo e Codecs**

| **Tecnologia**            | **Versão mínima**    | **Justificativa**                                                                       |
| ------------------------- | -------------------- | --------------------------------------------------------------------------------------- |
| AV1 com SVC               | libaom 3.x / SVT-AV1 | Encode único gera todas as layers de qualidade; operação de strip é bitwise sem decode  |
| H.264 Baseline (fallback) | libx264              | Compatibilidade com devices sem suporte AV1; produzido apenas da layer base L0          |
| HLS (HTTP Live Streaming) | RFC 8216             | Transporte de segmentos sobre HTTP padrão; compatível com CDN, caches e players nativos |
| fMP4 (Fragmented MP4)     | ISO 14496-12         | Container para segmentos HLS moderno; suporte a AV1 e metadados de timing               |

## **4.2 Rede P2P e Descoberta**

| **Tecnologia** | **Implementação**     | **Uso no sistema**                                                                        |
| -------------- | --------------------- | ----------------------------------------------------------------------------------------- |
| DHT Kademlia   | libp2p/kad-dht        | Descoberta de peers, anúncio de streams, registro de capacidade de nós                    |
| WebRTC         | libwebrtc             | Conexões diretas entre nós para entrega de chunks; NAT traversal automático via ICE       |
| ICE/STUN/TURN  | coturn (TURN server)  | NAT traversal - STUN para descoberta de IP público, TURN como relay de último recurso     |
| libp2p         | rust-libp2p           | Framework P2P com multiplexing, encryption (Noise Protocol), e transports (QUIC/TCP)      |
| QUIC           | RFC 9000              | Transporte primário entre nós; 0-RTT reconnection, multiplexing sem head-of-line blocking |

## **4.3 Sincronização e Confiabilidade**

| **Tecnologia**                | **Especificação**    | **Uso no sistema**                                                                     |
| ----------------------------- | -------------------- | -------------------------------------------------------------------------------------- |
| PTP (Precision Time Protocol) | IEEE 1588-2019       | Sincronização de relógio sub-milissegundo entre nós para playout buffer determinístico |
| Reed-Solomon Erasure Coding   | reed-solomon-erasure (Rust crate) | Fragmentação de chunks com paridade para tolerância a falhas de nós            |
| NACK Seletivo                 | RFC 4585 / RTP       | Recuperação de pacotes perdidos entre nós com timeout baseado no deadline global       |

## **4.4 Criptografia e Identidade**

| **Tecnologia**      | **Especificação**         | **Uso no sistema**                                                                                |
| ------------------- | ------------------------- | ------------------------------------------------------------------------------------------------- |
| Ed25519             | RFC 8032                  | Assinatura de chunks de vídeo, mensagens de chat e registros DHT                                  |
| SHA-256             | FIPS 180-4                | Hash de chunks para deduplicação, IDs de mensagem de chat, derivação de stream_id                 |
| Noise Protocol (XX) | Noise Spec rev 34         | Encryption de canais P2P entre nós via libp2p                                                     |
| State Channels      | Lightning-style off-chain | Micropagamentos por segundo de stream entregue; liquidação on-chain apenas no fechamento do canal |

## **4.5 Chat**

| **Tecnologia**            | **Tipo**              | **Uso no sistema**                                                   |
| ------------------------- | --------------------- | -------------------------------------------------------------------- |
| Gossip Epidemic Broadcast | Algoritmo distribuído | Propagação de mensagens com cobertura O(log N) e fanout=3            |
| Vector Clocks             | Algoritmo distribuído | Ordenação causal de mensagens concorrentes sem servidor              |
| Prev-Hash Chain           | Estrutura de dados    | Garantia de entrega causal e verificação de integridade da sequência |
| LRU Cache (hash de msgs)  | Estrutura de dados    | Deduplicação de mensagens recebidas por múltiplos caminhos gossip    |

# **5\. Roadmap de Desenvolvimento**

O roadmap é dividido em 4 fases com entregáveis claros e critérios de conclusão mensuráveis. Cada fase constrói sobre a anterior e pode ser testada independentemente.

## **Fase 1 - Fundação P2P (Meses 1-3)**

_Meta: rede P2P funcional com descoberta via DHT e transferência de arquivos verificada criptograficamente entre nós._

| **Entregável**                     | **Descrição**                                                                              | **Critério de conclusão**                            |
| ---------------------------------- | ------------------------------------------------------------------------------------------ | ---------------------------------------------------- |
| Nó base P2P                        | Implementação da DHT Kademlia via libp2p com operações FIND_NODE, STORE, FIND_VALUE        | 2 nós se descobrem sem servidor                      |
| Identidade criptográfica           | Geração de chaves Ed25519, assinatura e verificação de mensagens arbitrárias               | Verificação de assinatura entre nós distintos        |
| Transferência de chunks verificada | Envio de arquivo arbitrário com assinatura Ed25519 e verificação no receptor               | Chunk corrompido detectado e descartado              |
| NAT traversal                      | Integração ICE/STUN com coturn; teste de conectividade entre nós em NAT distintos          | Conexão estabelecida entre 2 nós em redes diferentes |
| Health score e vnodes              | Benchmark de capacidade no startup, cálculo de peso, anúncio na DHT, atualização periódica | DHT reflete capacidade real do nó                    |

## **Fase 2 - Pipeline de Vídeo (Meses 4-7)**

_Meta: stream de vídeo ao vivo fluindo do streamer até viewers através da rede P2P com latência dentro da janela de 7-20s._

| **Entregável**                 | **Descrição**                                                                                            | **Critério de conclusão**                       |
| ------------------------------ | -------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| Ingest e segmentação           | Cliente captura A/V, encode AV1/SVC via SVT-AV1, segmenta em chunks de 3s, assina e injeta em 4 nós seed | Chunks chegam nos 4 seeds com assinatura válida |
| Topologia de árvore + failover | Fanout máximo de 8, dual-parent WebRTC estabelecido, failover automático quando nó cai                   | Failover em <1s sem interrupção do stream       |
| Classes de nós e pipeline      | Nós Classe A fazem remux HLS; Classe B fazem transcoding L0→H.264; layer stripping no edge               | Viewer recebe HLS válido em player padrão       |
| Reed-Solomon                   | Implementação RS(10,4) para fragmentação de chunks em nós Classe C; reconstrução funcional               | Chunk reconstruído com quaisquer 10 de 14 fragmentos |
| PTP + playout buffer           | Sincronização de relógio PTP entre nós, cálculo de deadline global, buffer determinístico                | Drift entre viewers < 200ms medido              |
| NACK seletivo inter-nós        | Detecção de gap em sequence numbers, NACK propagado upstream, timeout baseado em deadline                | Taxa de frame loss < 0.1% em rede estável       |

## **Fase 3 - Chat e Identidade (Meses 8-10)**

_Meta: chat em tempo real funcionando sobre a mesma rede P2P, com ordenação causal e histórico para late joiners._

| **Entregável**            | **Descrição**                                                                                                | **Critério de conclusão**                          |
| ------------------------- | ------------------------------------------------------------------------------------------------------------ | -------------------------------------------------- |
| Gossip broadcast          | Implementação de epidemic broadcast com fanout=3, cache LRU de hashes, deduplicação                          | Cobertura >99% em rede de 100 nós em <500ms        |
| Vector clocks + prev-hash | Estruturas de causalidade, buffer de reordenação com TTL de 30s, ordenação determinística de concorrentes    | Zero casos de resposta antes de pergunta em testes |
| Anchor nodes              | Eleição de anchors via uptime+storage na DHT, armazenamento de 500 msgs, API de histórico para novos viewers | Late joiner recebe histórico em <2s                |
| Moderação distribuída     | Blocklist assinada Ed25519 propagada via gossip, aplicação local no cliente, score de reputação              | Blocklist propagada para todos em <1s              |
| UI de chat integrada      | Interface no cliente web/desktop exibindo chat em tempo real com identificação por pubkey e display name     | Chat funcional end-to-end no cliente               |

## **Fase 4 - Produção e Resiliência (Meses 11-14)**

_Meta: sistema estável sob carga real, com monitoramento distribuído, state channels funcionais e cliente polido para streamer e viewer._

| **Entregável**              | **Descrição**                                                                                      | **Critério de conclusão**                |
| --------------------------- | -------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| State channels (pagamentos) | Abertura/fechamento de canal off-chain, micropagamentos por segundo de stream, liquidação on-chain | Ciclo completo open→stream→close testado |
| Monitoramento distribuído   | Telemetria por nó agregada via gossip (sem servidor central), dashboards de health da rede         | Métricas visíveis sem servidor de coleta |
| Cliente streamer polido     | UI de configuração de stream, visualização de viewers, earnings, qualidade de rede em tempo real   | Streamer inicia live em <30s do zero     |
| Cliente viewer polido       | Descoberta de streams, player com ABR visual, chat integrado, configurações de qualidade           | Viewer assiste sem configuração manual   |
| Testes de carga e caos      | Simulação de 60% de nós caindo simultaneamente, spike de viewers, partições de rede                | Todos os RNFs validados sob carga        |

# **6\. Plano de Implementação**

## **6.1 Estrutura do Repositório**

prism/

├── Cargo.toml # Cargo workspace root

├── proto/ # Protobuf definitions (shared)

│ ├── node_record.proto

│ ├── video_chunk.proto

│ ├── chat_message.proto

│ └── ...

└── crates/

├── prism-core/ # Identidade Ed25519, SHA-256 (shared)

├── prism-proto/ # Código gerado pelo prost (shared)

├── prism-node/ # Nó da rede (Rust + Tokio)

│ ├── src/dht/ # Kademlia DHT via rust-libp2p

│ ├── src/relay/ # Topologia de árvore + failover QUIC

│ ├── src/classes/ # Classes A, B, C, edge

│ ├── src/erasure/ # Reed-Solomon via reed-solomon-erasure

│ ├── src/health/ # Benchmark, vnodes, health score

│ └── src/sync/ # NTP + playout buffer

├── prism-encoder/ # SVT-AV1 via FFI Rust

├── prism-ingest/ # Captura A/V, segmentação, injeção DHT

├── prism-chat/ # Chat: gossipsub, vector clocks, anchors

├── prism-payments/ # State channels off-chain

├── prism-telemetry/ # Telemetria distribuída via gossip

├── prism-studio/ # Cliente do streamer (Tauri 2 + React)

└── prism-player/ # Cliente do viewer (PWA React + React Native)

## **6.2 Linguagens e Frameworks**

| **Componente**   | **Linguagem / Framework** | **Justificativa**                                                                    |
| ---------------- | ------------------------- | ------------------------------------------------------------------------------------ |
| Nó da rede       | Rust + Tokio async        | Sem GC pauses, rust-libp2p mais ativo, FFI limpo com SVT-AV1                        |
| Cliente streamer | Rust + Tauri 2 (desktop)  | Reutiliza crates do core, binários < 10MB, suporte iOS/Android, < 40MB RAM           |
| Cliente viewer   | TypeScript + React (PWA) + React Native | HLS.js para player, web-native, app nativo para iOS/Android         |
| Encode AV1       | SVT-AV1 (C) via FFI Rust  | Encoder AV1 mais rápido com suporte a SVC; FFI direto sem overhead de bridge         |
| Protobuf         | Protocol Buffers v3 + prost | Serialização eficiente com geração de código Rust automática via build.rs           |

## **6.3 Definição dos Protobuf Principais**

// chunk.proto - chunk de vídeo

message VideoChunk {

string stream_id = 1; // SHA256(streamer_pubkey + start_ts)

uint64 sequence = 2; // número sequencial do chunk

uint64 timestamp_ms = 3; // wall clock do início do segmento

bytes payload = 4; // fMP4 com AV1/SVC, todas as layers

bytes streamer_sig = 5; // Ed25519 sobre hash(payload)

string prev_chunk_id = 6; // hash do chunk anterior

}

// chat.proto - mensagem de chat

message ChatMessage {

string id = 1; // SHA256(sender + text + timestamp)

bytes sender_pubkey = 2; // Ed25519 pubkey (32 bytes)

string text = 3; // conteúdo (max 500 chars)

uint64 timestamp_ms = 4; // wall clock Unix ms

string prev_msg_id = 5; // último msg_id que o sender viu

bytes signature = 6; // Ed25519Sign(privkey, hash(1..5))

}

// node_record.proto - anúncio de nó na DHT

message NodeRecord {

bytes node_id = 1; // SHA256(pubkey)

bytes pubkey = 2;

string region = 3; // ex: "sa-east-1"

string capacity_class = 4; // "A", "B", "C", "edge"

uint32 available_slots = 5;

float health_score = 6;

uint64 expires_at = 7; // Unix ms - TTL de 10s

bytes signature = 8; // assinatura do próprio nó

}

## **6.4 Sequência de Implementação Detalhada**

### **Sprint 1-2 (Semanas 1-4): Core P2P**

- Implementar DHT Kademlia com operações básicas usando rust-libp2p/kad-dht
- Implementar geração de chaves Ed25519 e funções sign/verify (ed25519-dalek)
- Implementar descoberta de peers via bootstrap list + DHT
- Testes: 10 nós em localhost se descobrem e roteiam corretamente

### **Sprint 3-4 (Semanas 5-8): Transporte e Identidade**

- Integrar QUIC (quinn) como transporte primário via rust-libp2p
- Implementar coturn para STUN/TURN; testar NAT traversal entre VMs em redes distintas
- Implementar registro NodeRecord na DHT com TTL e renovação automática
- Implementar benchmark de capacidade no startup e cálculo de vnodes

### **Sprint 5-7 (Semanas 9-14): Ingest e Segmentação**

- Integrar SVT-AV1 via FFI Rust (bindgen); implementar encode SVC com 4 layers
- Implementar segmentador de chunks de 3s em fMP4
- Implementar assinatura Ed25519 por chunk e injeção em 4 nós seed via DHT
- Testes: chunk flui do streamer para 4 seeds com verificação de assinatura

### **Sprint 8-10 (Semanas 15-20): Topologia e Relay**

- Implementar topologia de árvore com fanout=8 e gestão de filhos
- Implementar dual-parent: conexão backup estabelecida, ativação automática no failover
- Implementar Classes A (remux HLS), B (transcode L0→H.264), edge (layer strip)
- Testes end-to-end: stream flui do host até viewer em player HLS padrão

### **Sprint 11-12 (Semanas 21-24): Confiabilidade**

- Implementar Reed-Solomon RS(10,4) para fragmentação em nós Classe C
- Implementar PTP sync entre nós e playout buffer determinístico
- Implementar NACK seletivo inter-nós com timeout baseado em deadline
- Testes de caos: 60% dos nós removidos abruptamente - stream não interrompe

### **Sprint 13-15 (Semanas 25-30): Chat**

- Implementar gossip epidemic com fanout=3 e cache LRU de hashes
- Implementar vector clocks e buffer de reordenação com TTL=30s
- Implementar prev-hash chain e validação de causalidade
- Implementar anchor nodes: eleição, armazenamento de 500 msgs, API de histórico
- Testes: cobertura >99% em rede de 100 nós em <500ms medido

### **Sprint 16-18 (Semanas 31-36): Clientes e Produção**

- Implementar UI do streamer: configuração, métricas, earnings
- Implementar UI do viewer: discovery de streams, player ABR, chat integrado
- Implementar state channels para micropagamentos
- Testes de carga: 10.000 viewers simultâneos em testnet de 50 nós
- Validação de todos os RNFs listados na seção 3.4

# **7\. Riscos Técnicos e Mitigações**

| **Risco**                                                           | **Probabilidade** | **Mitigação**                                                                                                                                       |
| ------------------------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| Poucos nós em regiões específicas causam latência >20s              | **Alta**          | RS(4,2) em redes pequenas; nós seed em múltiplas regiões operados pela equipe na fase inicial; incentivo econômico para nós em regiões sub-servidas |
| NAT traversal falha para nós em CGNAT (móvel, ISPs restritivos)     | **Média**         | TURN server de último recurso (coturn); relay transparente sem impacto na arquitetura do resto da rede                                              |
| SVT-AV1 SVC muito lento para encode em tempo real em hardware fraco | **Média**         | Fallback para H.264 no streamer com SVC simulado (2 qualidades apenas); encode acelerado via VAAPI/NVENC quando disponível                          |
| Churn alto de nós fragmenta a árvore de relay                       | **Baixa**         | Dual-parent elimina downtime na falha de nó único; consistente hashing minimiza redistribuição ao entrar/sair nós                                   |
| Spam de mensagens no chat satura o gossip                           | **Média**         | Rate limiting por pubkey nos peers (max 2 msg/s aceitas por remetente); mensagens além do rate são descartadas antes de propagar                    |

# **8\. Glossário**

| **Termo**                       | **Definição**                                                                                                                 |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| **AV1/SVC**                     | Codec de vídeo moderno com Scalable Video Coding - encode único gera múltiplas camadas de qualidade embutidas                 |
| **DHT Kademlia**                | Distributed Hash Table baseada no algoritmo Kademlia - tabela hash distribuída para descoberta de peers sem servidor central  |
| **Ed25519**                     | Algoritmo de assinatura digital de curva elíptica - rápido, compacto e seguro contra ataques de canal lateral                 |
| **Erasure Coding (RS)**         | Reed-Solomon: técnica de codificação que fragmenta dados com paridade, permitindo reconstrução mesmo com perda de fragmentos  |
| **Gossip / Epidemic Broadcast** | Protocolo de disseminação de informação onde cada nó repassa para K peers aleatórios - cobertura total em O(log N) rounds     |
| **HLS**                         | HTTP Live Streaming - protocolo Apple para streaming adaptativo via segmentos .ts e manifests .m3u8 sobre HTTP padrão         |
| **Layer Stripping**             | Remoção de camadas SVC de um stream AV1 sem decodificação - operação bitwise de custo desprezível                             |
| **NACK Seletivo**               | Negative Acknowledgement - sinalização de pacote perdido para retransmissão seletiva, sem retransmitir toda a janela          |
| **Playout Buffer**              | Buffer de reprodução que segura frames até um deadline determinístico, eliminando drift temporal entre viewers                |
| **PTP**                         | Precision Time Protocol (IEEE 1588) - sincronização de relógio de precisão sub-milissegundo entre máquinas em rede            |
| **QUIC**                        | Protocolo de transporte sobre UDP com multiplexing, criptografia built-in e 0-RTT reconnection                                |
| **Reed-Solomon**                | Ver Erasure Coding. Parâmetros RS(n,k): n fragmentos totais, k necessários para reconstrução                                  |
| **State Channels**              | Canais de pagamento off-chain - transações ocorrem fora da blockchain com liquidação final on-chain apenas no fechamento      |
| **Vector Clock**                | Estrutura de dados para rastrear causalidade em sistemas distribuídos - vetor de contadores por participante                  |
| **Vnode**                       | Nó virtual no anel de consistent hashing - nós com mais capacidade possuem mais vnodes e recebem proporcionalmente mais carga |

_Prism v1.0 - Documento vivo. Atualizações refletem decisões de design tomadas durante o desenvolvimento._