# **9\. Clientes - Streamer e Viewer**

Esta seção documenta os dois clientes de usuário final do Prism: o Prism Studio (cliente do streamer) e o Prism Player (cliente do viewer). Para cada um são especificados arquitetura técnica, UX, aparência visual, fluxos de interação e requisitos de plataforma, incluindo possibilidade de execução via web e mobile.

## **9.1 Visão Geral e Responsabilidades**

| **Cliente**               | **Responsabilidade Principal**                                 | **Usuário Alvo**                            |
| ------------------------- | -------------------------------------------------------------- | ------------------------------------------- |
| **Prism Studio** | Captura A/V, encode, ingest na rede P2P, gestão da live        | Streamer - desktop (Windows/macOS/Linux)    |
| **Prism Player** | Descoberta de streams, reprodução adaptativa, chat, identidade | Viewer - web, desktop, mobile (iOS/Android) |

_Os dois clientes são independentes mas compartilham os módulos de identidade (Ed25519), DHT e chat. Um streamer usa o Studio; quem só assiste usa o Player. O mesmo usuário pode usar ambos._

## **9.2 Prism Studio - Cliente do Streamer**

### **9.2.1 Arquitetura Técnica do Studio**

O Studio é uma aplicação desktop nativa construída com Tauri 2 (Rust + WebView2/WebKit). O backend em Rust executa toda a lógica pesada: captura A/V, encode AV1, segmentação, assinatura e ingest P2P — reutilizando as crates do core P2P. O frontend React serve apenas como camada de UI - renderiza métricas e recebe eventos do backend via IPC bidirecional.

| **Camada**    | **Tecnologia**                   | **Responsabilidade**                                                         |
| ------------- | -------------------------------- | ---------------------------------------------------------------------------- |
| Backend (Rust) | Rust + Tauri 2 runtime          | Captura A/V, encode SVT-AV1, segmentação, DHT, ingest P2P                   |
| Captura A/V   | nokhwa (Rust) / RTMP input       | Acesso a câmeras, microfones, captura de tela/janela                         |
| Encoder       | SVT-AV1 via FFI Rust (bindgen)   | Encode AV1/SVC em tempo real com aceleração de hardware                      |
| Frontend (UI) | React + TypeScript               | Dashboard de métricas, configuração, chat, alertas                           |
| IPC           | Tauri commands + events          | Backend emite métricas via Tauri events; UI invoca comandos via invoke()     |
| Plugin OBS    | Servidor RTMP local porta 1935   | Studio levanta servidor RTMP; OBS envia stream via Custom RTMP output        |

### **9.2.2 Integração com OBS e Ferramentas Existentes**

Streamers profissionais utilizam OBS Studio, Streamlabs, ou XSplit para compor cenas, gerenciar fontes e aplicar filtros visuais. Prism Studio não substitui essas ferramentas - integra-se a elas como destino de saída.

**Modo 1 - Studio como destino RTMP local**

OBS envia stream via RTMP para o Studio rodando na mesma máquina como servidor RTMP local. Este é o modo mais simples e funciona com qualquer software de streaming existente sem modificações.

┌─────────────────────┐ RTMP local ┌───────────────────────────┐

│ OBS / Streamlabs │ ──────────────► │ Prism Studio │

│ (composição de │ rtmp://localhost │ (recebe RTMP, re-encoda │

│ cenas, filtros) │ :1935/live │ AV1/SVC, injeta na rede)│

└─────────────────────┘ └───────────────────────────┘

- Streamer configura no OBS: Settings → Stream → Custom → Server: rtmp://localhost:1935/live
- Studio levanta servidor RTMP na porta 1935 automaticamente ao iniciar
- Studio recebe o stream H.264/H.265 do OBS, re-encoda para AV1/SVC e injeta na rede P2P
- Latência adicional do re-encode: ~200-400ms - dentro da janela de 7-20s
- Resolução e bitrate do OBS são respeitados - Studio adapta o encode conforme o input

**Modo 2 - Studio como plugin nativo do OBS (longo prazo, Fase 4+)**

Plugin OBS em C++ que expõe o Studio como um "Output" nativo dentro do OBS. O stream vai diretamente do pipeline interno do OBS para o encoder AV1 do Studio sem passar por RTMP.

- Elimina o overhead de RTMP e o re-encode duplo
- Aparece como opção em OBS: Tools → Prism → Go Live
- Configuração de chave, qualidade e visibilidade da stream sem sair do OBS
- Recebe métricas de rede (viewers, bitrate, health) diretamente no OBS como dock panel

_⚠ O plugin nativo é complexo de manter para múltiplas versões do OBS. Recomendado apenas após o Modo 1 estar estável e validado em produção._

**Compatibilidade com outras ferramentas**

| **Ferramenta**          | **Método de integração**              | **Notas**                                                      |
| ----------------------- | ------------------------------------- | -------------------------------------------------------------- |
| OBS Studio              | RTMP local (Modo 1) / Plugin (Modo 2) | Principal caso de uso; todas as versões compatíveis com RTMP   |
| Streamlabs Desktop      | RTMP local                            | Mesmo mecanismo do OBS - mesma configuração                    |
| XSplit Broadcaster      | RTMP local                            | Suporta Custom RTMP output                                     |
| FFmpeg CLI              | RTMP local                            | ffmpeg ... -f flv rtmp://localhost:1935/live                   |
| Captura nativa (Studio) | Built-in no Studio                    | Para streamers simples que não precisam de composição avançada |

### **9.2.3 Layout e UX do Studio**

O Studio segue o paradigma visual de ferramentas profissionais de broadcast (inspirado no OBS e no XSplit) mas simplificado para focar no que é específico do Prism: saúde da rede P2P, distribuição de nós e estado da stream.

**Layout principal - janela única com 4 zonas**

╔══════════════════════════════════════════════════════════════════════════╗

║ Prism Studio \[─\] \[□\] \[×\] ║

╠══════════╦═══════════════════════════════════════╦═══════════════════════╣

║ ║ ║ NETWORK HEALTH ║

║ SOURCES ║ PREVIEW ║ ┌─────────────────┐ ║

║ ────── ║ ┌─────────────────────────────┐ ║ │ Nodes: 47 active│ ║

║ □ Camera║ │ │ ║ │ Viewers: 1,204 │ ║

║ □ Screen║ │ \[live preview\] │ ║ │ Bitrate: 4.2Mbps│ ║

║ □ Window║ │ │ ║ │ Latency: ~11s │ ║

║ □ Image ║ └─────────────────────────────┘ ║ └─────────────────┘ ║

║ □ Audio ║ 1920×1080 · 59fps ║ ║

║ ║ ║ NODE TREE ║

║ \[+ Add\] ║ ────────────────────────────── ║ ┌─────────────────┐ ║

║ ║ ENCODE AV1/SVC · L0-L3 · 5.8Mbps ║ │ seed-br-01 ● │ ║

╠══════════╩═══════════════════════════════════════╣ │ ├ relay-01 ● │ ║

║ ║ │ ├ relay-02 ● │ ║

║ \[● GO LIVE\] \[■ END STREAM\] \[⚙ Settings\] ║ │ └ relay-03 ● │ ║

║ ║ └─────────────────┘ ║

╚══════════════════════════════════════════════════╩═══════════════════════╝

**Zona 1 - Sources (painel esquerdo)**

- Lista de fontes de captura ativas: câmera, captura de tela, janela específica, imagem estática, dispositivo de áudio
- Cada fonte tem toggle on/off, ícone de status e indicador de carga CPU
- Botão \[+ Add\] abre modal de seleção de fonte com detecção automática de dispositivos disponíveis
- Drag-and-drop para reordenar camadas (composição básica sem precisar do OBS para casos simples)

**Zona 2 - Preview (centro)**

- Preview ao vivo do que está sendo capturado - atualizado a 15fps para economizar CPU
- Indicador de resolução e framerate atual do encode
- Barra de encode: codec ativo, layers SVC habilitadas, bitrate atual
- Overlay de warning quando CPU > 80% ou bitrate instável

**Zona 3 - Network Health (painel direito)**

- Contador de nós ativos na rede para esta stream (atualizado a cada 5s via DHT)
- Contador de viewers conectados (estimativa baseada em pull de manifests HLS)
- Bitrate efetivo de saída (host → seeds)
- Latência estimada streamer → viewer (baseada no playout buffer configurado)
- Node Tree: visualização da árvore de relay com status de cada nó (verde/amarelo/vermelho)
- Clique em nó na tree expande métricas: região, classe, health_score, viewers servidos

**Zona 4 - Barra inferior de controle**

- \[● GO LIVE\] - inicia a stream. Antes de ativar, abre diálogo de confirmação com título, visibilidade (pública/privada) e configuração de qualidade
- \[■ END STREAM\] - encerra a stream. Abre confirmação com resumo: duração, peak viewers, bytes transmitidos
- \[⚙ Settings\] - abre painel de configurações (detalhado abaixo)
- Durante live: botão GO LIVE muda para indicador de tempo ao vivo pulsante em vermelho

**Painel de Configurações do Studio**

| **Aba**        | **Configurações disponíveis**                                                                                                               |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| **Identidade** | Chave pública (exibida como QR code e texto), display name, exportar/importar chave privada (ZIP criptografado)                             |
| **Encode**     | Preset de qualidade (Baixo/Médio/Alto/Ultra), framerate máximo, bitrate máximo de upload, aceleração de hardware (VAAPI/NVENC/VideoToolbox) |
| **Rede**       | Latência alvo (slider 7-20s), número de seeds iniciais (padrão 4), configuração STUN/TURN custom, porta do servidor RTMP local              |
| **Stream**     | Título padrão da live, visibilidade padrão (pública/privada), blocklist de viewers (por chave pública)                                      |
| **Avançado**   | Parâmetros Reed-Solomon (n,k), tamanho do chunk (padrão 3s), logs de debug, modo de diagnóstico de rede                                     |

### **9.2.4 Fluxo de Primeiro Uso - Onboarding do Studio**

O primeiro acesso deve ser completado em menos de 2 minutos sem conhecimento técnico de P2P ou criptografia.

- Passo 1 - Instalação: download de executável único (~80MB) sem dependências externas. Windows: .exe installer. macOS: .dmg. Linux: AppImage.
- Passo 2 - Geração de identidade: na primeira execução, Studio gera automaticamente o par Ed25519 e exibe a chave pública como um endereço legível (ex: ds1a3f7...b92e). Popup explica o que é e oferece backup.
- Passo 3 - Teste de rede: Studio executa benchmark automático de CPU/banda e configura classe de nó e preset de encode recomendado. Mostra: "Sua conexão suporta stream em 1080p60."
- Passo 4 - Configurar fonte: wizard guiado para selecionar câmera, microfone ou OBS como fonte.
- Passo 5 - GO LIVE: botão principal. Na primeira vez, exibe tutorial overlay com tooltips nas 4 zonas.

### **9.2.5 Chat no Studio**

O Studio exibe o chat da live em um painel flutuante redimensionável que pode ser ancorado à direita da janela ou destacado como janela separada (útil para streamers com segundo monitor).

- Exibe mensagens em tempo real com display name e avatar gerado por hash da pubkey (identicon)
- Streamer pode fixar mensagem no topo do chat (anunciada via gossip como mensagem especial)
- Clique em usuário abre menu: Ver perfil / Adicionar à blocklist / Nomear moderador
- Moderadores recebem permissão de gerenciar blocklist em nome do streamer (blocklist assinada pelo moderador, referenciada na blocklist do streamer)
- Campo de resposta rápida do streamer com atalho de teclado configurável

## **9.3 Prism Player - Cliente do Viewer**

### **9.3.1 Plataformas Suportadas**

O Player é a interface que qualquer pessoa usa para assistir streams. Diferente do Studio que é desktop-only por necessidade de hardware, o Player pode rodar em qualquer dispositivo.

| **Plataforma**              | **Implementação**                         | **Capacidades e limitações**                                                                                                           |
| --------------------------- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| **Web (browser)**           | React PWA + HLS.js + WebRTC               | Plataforma primária. Funciona em qualquer browser moderno sem instalação. Suporte completo a chat, identidade e descoberta.            |
| **Desktop (Win/Mac/Linux)** | Tauri 2 + mesmo código React              | Wrapper nativo com notificações nativas, proteção de chave no keychain do OS, e capacidade de ser nó relay. Binário < 10MB, RAM idle 30–40MB.  |
| **Mobile iOS**              | React Native + expo-av + HLS nativo       | HLS suportado nativamente pelo AVPlayer. WebRTC via react-native-webrtc. Chat completo. Sem ser nó relay (limitação de bateria/dados). |
| **Mobile Android**          | React Native + ExoPlayer + HLS            | ExoPlayer com suporte a AV1 (Android 10+). Fallback H.264 para Android mais antigo. Mesmo conjunto de features do iOS.                 |
| **TV (SmartTV / AppleTV)**  | Versão web adaptada para TV (longo prazo) | Layout 10-foot UI. Navegação por controle remoto/D-pad. Chat simplificado (somente leitura ou via paired phone).                       |

### **9.3.2 Arquitetura Técnica do Player Web**

O Player web é uma PWA (Progressive Web App) que funciona como aplicação instalável no browser sem necessidade de loja de aplicativos. A arquitetura separa reprodução de vídeo, chat e identidade em módulos independentes.

| **Módulo**            | **Tecnologia**                   | **Responsabilidade**                                                        |
| --------------------- | -------------------------------- | --------------------------------------------------------------------------- |
| Player de vídeo       | HLS.js + Media Source Extensions | Playback HLS adaptativo, ABR automático, buffer management                  |
| P2P delivery          | WebRTC Data Channels             | Receber segmentos de nós edge próximos via WebRTC em vez de HTTP puro       |
| Chat engine           | WebRTC + gossip JS               | Participar da rede gossip, enviar/receber mensagens, vector clocks          |
| Identidade            | WebCrypto API (Ed25519)          | Geração e uso de chaves Ed25519 nativas do browser via SubtleCrypto         |
| DHT client            | js-libp2p/kad-dht                | Descoberta de streams e nós edge via DHT - versão leve, só consulta         |
| Persistência de chave | IndexedDB (criptografado)        | Chave privada armazenada encriptada no browser; senha definida pelo usuário |

**Nota sobre Web Crypto e Ed25519**

_A WebCrypto API suporta Ed25519 desde 2022 (Chrome 113+, Firefox 119+, Safari 17+). Para browsers mais antigos, fallback via biblioteca noble/ed25519 em WebAssembly._

### **9.3.3 Layout e UX do Player Web**

O Player segue o paradigma de plataformas de streaming modernas (referência visual: Twitch/YouTube Live) com adaptações para o contexto descentralizado: indicadores de rede visíveis e identidade por chave.

**Layout Desktop - tela de stream ativa**

╔══════════════════════════════════════════════════════════════════════════════╗

║ Prism \[🔍 Descobrir\] \[👤 Minha identidade\] \[⚙\] ║

╠══════════════════════════════════════════════════╦═══════════════════════════╣

║ ║ CHAT ║

║ ║ ───────────────────── ║

║ ║ 🟢 alice7f3 12:04:32 ║

║ \[ V Í D E O P L A Y E R \] ║ oi galera! ║

║ ║ ║

║ ║ 🟡 bob2a9 12:04:35 ║

║ ║ que stream boa ║

║ ────────────────────────────────────────────── ║ ║

║ ▶ ████████████████░░░░░░░ 01:23:47 🔊 ⛶ ║ 🟢 carol_k 12:04:38 ║

║ 720p ▾ \[ABR Auto\] ║ primero aqui ║

╠══════════════════════════════════════════════════║ ║

║ 🔴 AO VIVO ● 1,847 viewers · 47 nós ║ ───────────────────── ║

║ CryptoGamer · ds1a3f...b92e ║ \[ Digite uma msg... \] ║

╚══════════════════════════════════════════════════╩═══════════════════════════╝

**Elementos da interface do Player**

- Header: logo, barra de descoberta de streams, acesso à identidade do viewer, configurações
- Player de vídeo: ocupa ~70% da largura em desktop; controles padrão (play/pause, volume, fullscreen, qualidade)
- Barra de qualidade: mostra qualidade atual (ex: 720p), botão ABR Auto que permite override manual
- Barra de status da stream: indicador AO VIVO pulsante, contador de viewers, contador de nós ativos na rede
- Identidade do streamer: display name + primeiros e últimos 4 chars da chave pública (ex: ds1a3f...b92e)
- Chat: painel direito com scroll contínuo, identicon colorido por pubkey, timestamp relativo
- Campo de mensagem: ativo apenas se viewer tem identidade configurada; placeholder indica como criar uma

**Indicador de saúde da rede - elemento exclusivo vs plataformas centralizadas**

Um pequeno indicador no canto inferior do player mostra a saúde da rede P2P para aquela stream. Isso é educativo e cria transparência - algo impossível em plataformas centralizadas.

┌─────────────────────────────────────────┐

│ 🌐 Rede P2P │

│ Nós ativos: 47 Sua qualidade: 720p │

│ Nó mais próximo: sa-east-1 (~18ms) │

│ ████████████████░░░░ Boa conexão │

└─────────────────────────────────────────┘

**Layout Mobile - tela de stream ativa**

┌────────────────────────┐

│ Prism \[≡\] │

├────────────────────────┤

│ │

│ \[ VIDEO PLAYER \] │

│ (16:9, fullwidth) │

│ │

│ 🔴 AO VIVO 1,847 v │

│ CryptoGamer │

├────────────────────────┤

│ CHAT │

│ alice7f3: oi galera! │

│ bob2a9: que stream boa │

│ carol_k: primero aqui │

│ │

│ \[ Digite msg... \] │

└────────────────────────┘

- Em mobile, o vídeo ocupa toda a largura da tela (sem barra lateral)
- Chat exibido abaixo do player com altura fixa e scroll independente
- Toque no vídeo: exibe controles sobrepostos (play/pause, qualidade, fullscreen) por 3 segundos
- Fullscreen: rotação automática para landscape, chat como overlay semi-transparente na borda direita
- Chat em fullscreen: toggle por swipe ou botão de balão flutuante

### **9.3.4 Tela de Descoberta de Streams**

Sem servidor central de indexação, a descoberta é feita consultando a DHT por streams ativas. O cliente mantém um cache local de streams conhecidas e as exibe ordenadas por relevância.

╔══════════════════════════════════════════════════════════════════════╗

║ 🔍 \[ Buscar por nome ou chave pública... \] ║

╠══════════════════════════════════════════════════════════════════════╣

║ AO VIVO AGORA ║

║ ───────────────────────────────────────────────────────────────── ║

║ ┌──────────┐ CryptoGamer ┌──────────┐ ArtisticVibes ║

║ │\[thumbnail│ "Jogando Elden Ring" │\[thumbnail│ "Pintando ao vivo"║

║ │ vídeo \] │ 🔴 1,847 viewers │ vídeo \] │ 🔴 203 viewers ║

║ └──────────┘ ds1a3f...b92e └──────────┘ ds9f2b...a13c ║

║ ║

║ ┌──────────┐ MusicSession ┌──────────┐ CodeWithMe ║

║ │\[thumbnail│ "Jazz improvisado" │\[thumbnail│ "Construindo API" ║

║ │ vídeo \] │ 🔴 89 viewers │ vídeo \] │ 🔴 341 viewers ║

║ └──────────┘ ds4e8c...f01a └──────────┘ ds7d3e...c88b ║

╚══════════════════════════════════════════════════════════════════════╝

- Busca por nome: consulta cache local de streams conhecidas + query na DHT por texto parcial do título
- Busca por chave pública: acesso direto à stream de qualquer streamer com chave conhecida
- Thumbnail: frame capturado automaticamente a cada 30s pelo nó seed e armazenado via DHT como miniatura JPEG 320×180
- Contador de viewers: estimativa baseada em pulls de manifests HLS nos edge nodes
- Streams privadas: não aparecem na descoberta - acesso apenas por link direto (chave pública + stream_id)
- \[Seguir streamer\]: armazena a chave pública localmente. Player notifica quando streamer inicia nova live (via polling DHT)

### **9.3.5 Tela de Identidade do Viewer**

O viewer não precisa criar conta para assistir - pode ser completamente anônimo. Para participar do chat, precisa de uma identidade local. Este fluxo deve ser o mais transparente possível.

**Viewer anônimo (sem identidade)**

- Pode assistir streams sem nenhuma configuração
- Chat visível mas campo de digitação mostra: "Crie uma identidade para participar do chat →"
- Nenhum dado é enviado para a rede

**Criação de identidade (primeiro acesso ao chat)**

- Passo 1: clique em "Criar identidade" - modal simples com campo de display name
- Passo 2: Studio gera par Ed25519 via WebCrypto API em background (<100ms)
- Passo 3: opcional - definir senha para criptografar a chave no IndexedDB
- Passo 4: exibe chave pública gerada com opção de exportar (arquivo .json criptografado)
- Passo 5: viewer está pronto para participar do chat

_A identidade é criada localmente no browser. Se o viewer limpar os dados do browser sem exportar, a identidade é perdida permanentemente. Este aviso é exibido de forma clara durante a criação._

## **9.4 Design System - Tokens e Componentes**

Studio e Player compartilham o mesmo design system para consistência visual. Tema escuro como padrão - ambiente típico de uso de streaming é com luz ambiente baixa.

### **9.4.1 Paleta de Cores**

| **Token**         | **Hex** | **Uso**                                                                      |
| ----------------- | ------- | ---------------------------------------------------------------------------- |
| \--bg-base        | #0F1117 | Fundo principal da aplicação                                                 |
| \--bg-surface     | #1A1D27 | Cards, painéis, sidebars                                                     |
| \--bg-elevated    | #252837 | Tooltips, dropdowns, modais                                                  |
| \--accent-primary | #7B5CF5 | Botões primários, links, indicadores ativos (roxo - diferencia de Twitch/YT) |
| \--accent-live    | #E53935 | Indicador AO VIVO, alertas críticos                                          |
| \--accent-success | #43A047 | Nó saudável, encode OK, conexão boa                                          |
| \--accent-warning | #FB8C00 | Nó degradado, bitrate instável, aviso                                        |
| \--text-primary   | #F0F0F5 | Texto principal                                                              |
| \--text-secondary | #8A8FA8 | Labels, metadados, timestamps                                                |
| \--border         | #2E3245 | Bordas de cards e divisores                                                  |

### **9.4.2 Tipografia**

| **Token**       | **Fonte**                  | **Tamanho**     | **Uso**                                 |
| --------------- | -------------------------- | --------------- | --------------------------------------- |
| \--font-display | Inter / system-ui          | 24-32px bold    | Títulos de seção, nome do streamer      |
| \--font-body    | Inter / system-ui          | 14-16px regular | Corpo de texto, descrições              |
| \--font-chat    | Inter / system-ui          | 13px regular    | Mensagens de chat - compacto            |
| \--font-mono    | JetBrains Mono / monospace | 12px regular    | Chaves públicas, IDs técnicos, métricas |
| \--font-metric  | Inter Numeric / tabular    | 18-24px bold    | Contadores (viewers, bitrate, nós)      |

### **9.4.3 Componentes Principais**

- NodeStatusDot: ponto colorido (verde/amarelo/vermelho) + tooltip com métricas do nó ao hover
- IdentityBadge: avatar identicon (determinístico por pubkey) + display name + pubkey abreviada (6 chars + ... + 4 chars)
- LiveBadge: pulsação CSS + texto "AO VIVO" em --accent-live com sombra glow
- QualitySelector: dropdown com layers disponíveis + opção ABR Auto com ícone de varinha mágica
- NetworkHealthBar: barra de progresso colorida (verde→amarelo→vermelho) com label de qualidade
- ChatMessage: linha compacta com identicon 20px, display name clicável, texto, timestamp relativo hover
- StreamCard: thumbnail 16:9 com badge AO VIVO, nome, viewer count, pubkey abreviada do streamer

## **9.5 Capacidades PWA e Mobile**

### **9.5.1 O Player como PWA**

O Player web é distribuído como PWA (Progressive Web App). Isso significa que o viewer pode "instalar" o Player no dispositivo sem passar pela App Store - o browser gerencia a instalação.

- Service Worker: cache de assets estáticos para funcionamento offline parcial (não é possível ver stream offline, mas a UI carrega instantaneamente)
- Web App Manifest: ícone, nome, tema escuro configurado - aparece como app nativa quando instalado
- Push Notifications (opcional): notificação quando streamer favorito entra ao vivo - requer permissão do usuário. Implementado via Notification API do browser (sem servidor push central - viewer consulta DHT periodicamente e dispara a notificação localmente)
- Instalação: banner "Instalar Prism" aparece após 3 visitas. Não intrusivo.

### **9.5.2 Limitações Técnicas por Plataforma**

| **Recurso**         | **Web Desktop**         | **iOS / Safari**          | **Android / Chrome**                |
| ------------------- | ----------------------- | ------------------------- | ----------------------------------- |
| HLS playback        | ✅ HLS.js + MSE         | ✅ AVPlayer nativo        | ✅ ExoPlayer / HLS.js               |
| AV1 decode          | ✅ Chrome 70+, FF 67+   | ✅ iOS 17+ (A17 chip)     | ✅ Android 10+ (SW), HW Android 12+ |
| WebRTC P2P          | ✅ Completo             | ⚠️ Limitado em background | ✅ Completo                         |
| Ed25519 (WebCrypto) | ✅ Chrome 113+, FF 119+ | ✅ Safari 17+             | ✅ Chrome 113+                      |
| IndexedDB (chave)   | ✅ Permanente           | ⚠️ Pode ser limpo pelo OS | ✅ Permanente                       |
| Background fetch    | ✅ Service Worker       | ❌ iOS limita SW          | ✅ Service Worker                   |
| Ser nó relay        | ✅ Wails desktop        | ❌ Não suportado          | ❌ Não recomendado                  |

### **9.5.3 Estratégia de App Nativo Mobile (React Native)**

Para melhor experiência em mobile, o Player é disponibilizado também como app nativo via React Native, compartilhando ~85% do código com a versão web.

- Módulo de vídeo: expo-av para iOS (AVPlayer nativo) + ExoPlayer para Android - suporte a HLS nativo sem HLS.js
- Identidade: chave privada armazenada no Secure Enclave (iOS) ou Android Keystore - mais seguro que IndexedDB
- Notificações push: Firebase Cloud Messaging para Android, APNs para iOS - o nó do viewer consulta DHT e envia push local sem servidor central de notificações
- Distribuição: App Store (iOS) e Google Play (Android). O fato de ser P2P e descentralizado não conflita com as políticas das lojas - o app apenas conecta a uma rede aberta
- PWA como alternativa: usuários que não querem instalar o app nativo podem usar a PWA com experiência praticamente equivalente em Android; iOS tem mais limitações mas funciona para consumo básico

## **9.6 Requisitos Funcionais - Adições ao PRD**

Os requisitos abaixo complementam o PRD da seção 3, cobrindo especificamente os clientes de usuário final.

### **RF-06: Studio - Cliente do Streamer**

- RF-06.1: Studio deve aceitar stream RTMP de OBS/Streamlabs em rtmp://localhost:1935/live sem configuração adicional
- RF-06.2: Studio deve re-encodar stream recebida via RTMP para AV1/SVC com aceleração de hardware quando disponível (VAAPI/NVENC/VideoToolbox)
- RF-06.3: Studio deve exibir número de viewers e nós ativos com atualização máxima de 5 segundos de delay
- RF-06.4: Studio deve permitir encerrar, pausar e reiniciar stream sem reiniciar o aplicativo
- RF-06.5: Studio deve exibir alerta visual quando: CPU > 85%, bitrate de upload instável por >10s, menos de 2 nós seeds conectados
- RF-06.6: Studio deve permitir backup e restauração de chave privada via arquivo .json criptografado por senha
- RF-06.7: Studio deve funcionar em Windows 10+, macOS 12+ e Ubuntu 20.04+ como executável único sem dependências externas

### **RF-07: Player - Cliente do Viewer**

- RF-07.1: Player web deve funcionar sem instalação em Chrome 113+, Firefox 119+, Safari 17+ e Edge 113+
- RF-07.2: Player deve trocar de qualidade de vídeo automaticamente (ABR) em resposta a mudanças de largura de banda sem interrupção do playback
- RF-07.3: Player deve permitir assistir stream sem criar identidade (modo anônimo)
- RF-07.4: Player deve criar identidade Ed25519 localmente sem contato com servidor externo
- RF-07.5: Player deve exibir histórico das últimas 500 mensagens de chat ao entrar em stream ativa
- RF-07.6: Player deve funcionar como PWA instalável em Android e como app nativo via App Store (iOS) e Google Play (Android)
- RF-07.7: Player deve exibir indicador de saúde da rede (nós ativos, qualidade de conexão) de forma não intrusiva
- RF-07.8: Player deve notificar viewer quando streamer seguido inicia live, sem servidor central de notificações

### **RF-08: Onboarding**

- RF-08.1: Streamer deve conseguir iniciar primeira live em menos de 5 minutos após instalação do Studio, sem conhecimento de P2P
- RF-08.2: Viewer deve conseguir assistir stream em menos de 30 segundos após abrir o Player, sem criar conta
- RF-08.3: Toda terminologia técnica de P2P e criptografia deve ser abstraída na UI - o usuário vê "sua identidade", não "sua chave Ed25519"

## **9.7 Plano de Implementação - Adições ao Roadmap**

Os clientes são desenvolvidos em paralelo com as fases de infraestrutura. A UI evolui junto com o backend - a cada fase, o cliente expõe o que o backend suporta naquele momento.

| **Fase**   | **Sprint**            | **Studio (streamer)**                                                                                           | **Player (viewer)**                                                                                            |
| ---------- | --------------------- | --------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| **Fase 1** | S1-S4 (meses 1-3)     | Setup Tauri 2 + React; tela de identidade; configurações básicas; servidor RTMP local embrionário               | Setup PWA React; tela de identidade; geração de chave WebCrypto; UI shell sem vídeo                            |
| **Fase 2** | S5-S12 (meses 4-7)    | Captura A/V nativa; encode AV1; preview da stream; ingest P2P; métricas de rede ao vivo                         | HLS.js player integrado; ABR automático; descoberta de streams básica; exibição de stream ativa                |
| **Fase 3** | S13-S15 (meses 8-10)  | Chat panel no Studio; moderação; node tree visual; integração OBS via RTMP testada e documentada                | Chat completo no Player; identidade + gossip no browser; anchor node para histórico; tela de descoberta polida |
| **Fase 4** | S16-S18 (meses 11-14) | Plugin OBS (opcional); settings polidas; onboarding wizard; testes de usabilidade; build multi-plataforma CI/CD | App React Native (iOS + Android); PWA install flow; notificações de live; testes de usabilidade mobile         |

_Prism v1.0 - Seção 9: Clientes. Apêndice ao documento principal de arquitetura._