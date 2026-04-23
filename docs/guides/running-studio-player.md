# Rodando o Prism — Guia Completo

Este guia cobre o setup completo em **Windows** e **Linux** para uma sessão de
streaming local com todas as features: seed node → Studio (streamer) → Player web
(viewer) → Player mobile (React Native/Expo).

---

## Pré-requisitos

### Rust + Cargo

```bash
# Linux/macOS
curl https://sh.rustup.rs -sSf | sh

# Windows: baixar rustup-init.exe em https://rustup.rs

rustup default stable
rustup update
```

### Node.js ≥ 18

```bash
# Linux (via nvm)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
nvm install 20

# Windows — via winget:
winget install OpenJS.NodeJS.LTS
```

### Tauri CLI + dependências nativas

**Linux (Debian/Ubuntu):**
```bash
sudo apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    build-essential \
    pkg-config
```

**Windows:**
- Instalar **Microsoft C++ Build Tools** ou Visual Studio com "Desktop development with C++"
- **WebView2** já está incluso no Windows 10/11 atualizado

### Expo CLI (apenas para o player mobile)

```bash
npm install -g expo-cli
```

---

## Estrutura de portas

| Porta | Serviço | Quem abre |
|-------|---------|-----------|
| `1935` | RTMP — OBS/ffmpeg envia vídeo aqui | Studio (Rust, `rtmp_server.rs`) |
| `4001` | P2P libp2p DHT/QUIC | prism-node |
| `4002` | Recepção de chunks TCP | prism-node |
| `5173` | Dev server do Studio (Vite) | npm |
| `5174` | Dev server do Player web (Vite) | npm |
| `8080` | HLS HTTP — manifests e segmentos | prism-node |

---

## Passo 1 — Compilar o workspace Rust

```bash
cd openstream   # raiz do repositório

cargo build -p prism-node
```

A primeira compilação demora alguns minutos (libp2p, axum, ed25519-dalek, gossipsub).

---

## Passo 2 — Criar a config do seed node

### Linux

```bash
mkdir -p /tmp/prism

cat > /tmp/prism/seed.toml <<EOF
key_path        = "/tmp/prism/seed.key"
listen_addr     = "/ip4/0.0.0.0/udp/4001/quic-v1"
bootstrap_peers = []
capacity_class  = "A"
EOF
```

### Windows (PowerShell)

```powershell
New-Item -ItemType Directory -Force -Path "$env:TEMP\prism"

@"
key_path        = "$env:TEMP\prism\seed.key"
listen_addr     = "/ip4/0.0.0.0/udp/4001/quic-v1"
bootstrap_peers = []
capacity_class  = "A"
"@ | Out-File -Encoding utf8 "$env:TEMP\prism\seed.toml"
```

---

## Passo 3 — Iniciar o seed node

Deixe este terminal aberto durante toda a sessão.

### Linux

```bash
cd openstream
RUST_LOG=info cargo run -p prism-node -- --config /tmp/prism/seed.toml
```

### Windows (PowerShell)

```powershell
cd openstream
$env:RUST_LOG = "info"
cargo run -p prism-node -- --config "$env:TEMP\prism\seed.toml"
```

Logs esperados na inicialização:
```
INFO prism_node: identity ready
INFO prism_node: edge HTTP server listening addr=0.0.0.0:8080
INFO prism_node: chunk receiver listening addr=0.0.0.0:4002
INFO prism_node: prism-node running
```

O nó integra gossipsub (chat) e DHT automaticamente — não há processo separado
para o prism-chat.

---

## Passo 4 — Instalar dependências npm

### Studio

```bash
cd openstream/crates/prism-studio
npm install
```

### Player web

```bash
cd openstream/crates/prism-player
npm install
```

### Player mobile (opcional)

```bash
cd openstream/crates/prism-player-mobile
npm install
```

---

## Passo 5 — Iniciar o Studio

Escolha **uma** das opções abaixo:

### Opção A — Dev mode (só browser, sem app nativo)

Útil para desenvolver a UI sem precisar do Tauri instalado. Chat e pipeline de
vídeo ficam em modo mock.

```bash
cd openstream/crates/prism-studio

# Linux
PRISM_SEED_ADDRS=127.0.0.1:4002 npm run dev

# Windows (PowerShell)
$env:PRISM_SEED_ADDRS = "127.0.0.1:4002"
npm run dev
```

Abrir no browser: **http://localhost:5173**

> **Neste modo** o `stream_id` exibido é fictício (`dev-mock-stream-id`) e nenhum
> dado real é enviado ao seed node.

### Opção B — App Tauri (pipeline real)

Requer as dependências nativas do Tauri instaladas (ver Pré-requisitos).

```bash
cd openstream/crates/prism-studio

# Linux
PRISM_SEED_ADDRS=127.0.0.1:4002 npx tauri dev

# Windows (PowerShell)
$env:PRISM_SEED_ADDRS = "127.0.0.1:4002"
npx tauri dev
```

### Fluxo de onboarding (primeiro uso)

Na primeira execução o Studio exibe um **wizard de 5 passos**:

| Passo | O que acontece |
|-------|---------------|
| 1 — Identity | Par Ed25519 gerado e `pubkey` exibida |
| 2 — Backup | Exportar backup criptografado do arquivo de chave |
| 3 — Benchmark | Mede upload para sugerir preset de qualidade (Low / Medium / High) |
| 4 — Source | Escolher câmera ou OBS/RTMP como fonte de vídeo |
| 5 — Go Live | Onboarding concluído — usuário pronto para transmitir |

O estado do onboarding é salvo em `prism_onboarding.json` na pasta de dados do
app. Na próxima abertura o wizard é pulado automaticamente.

Após o onboarding, ao clicar **GO LIVE**:
1. O servidor RTMP sobe na porta `1935`
2. O `stream_id` (SHA-256 hex de 64 chars) é exibido na tela
3. O **ChatPanel** fica disponível para moderar o chat ao vivo

---

## Passo 6 — Enviar vídeo para o Studio

### Com OBS Studio

```
Configurações → Stream:
  Serviço: Custom
  Servidor: rtmp://localhost/live
  Chave: qualquer string (ex: test)
```

Clicar **Iniciar transmissão** no OBS.

O Studio exibe o **OBSGuide** com instruções passo a passo caso seja o primeiro
uso com OBS.

### Com ffmpeg (sem OBS)

```bash
# Vídeo sintético: barra de cor + tom de 440 Hz
ffmpeg -re \
  -f lavfi -i testsrc=size=1280x720:rate=30 \
  -f lavfi -i sine=frequency=440:sample_rate=44100 \
  -c:v libx264 -preset veryfast -b:v 1000k \
  -c:a aac -b:a 128k \
  -f flv rtmp://localhost/live/test
```

---

## Passo 7 — Iniciar o Player web

Em outro terminal:

```bash
cd openstream/crates/prism-player
npm run dev
```

Abrir no browser: **http://localhost:5174**

### Descoberta de streams (DHT)

O Player web conecta-se à DHT automaticamente. Na aba **Discover** é exibida a
lista de streams ativos encontrados via Kademlia — não é necessário inserir o
`stream_id` manualmente.

Alternativamente, inserir a URL diretamente no campo de URL:

```
http://localhost:8080/<stream_id>/master.m3u8
```

Substituir `<stream_id>` pelo valor exibido no Studio.

### Identidade do viewer

Na aba **Identity** o viewer pode criar ou importar um par Ed25519 (via WebCrypto
API) para participar do chat com uma identidade verificável. O par de chaves é
armazenado localmente no `localStorage` — nunca enviado a nenhum servidor.

### Chat no Player

Com o stream carregado e identidade configurada, o **ChatView** fica disponível
ao lado do player. Mensagens são propagadas via gossipsub e chegam em tempo real,
independentemente do buffer de vídeo.

---

## Passo 8 — Player mobile (opcional)

O app mobile usa **React Native + Expo** e conecta ao mesmo seed node local.

```bash
cd openstream/crates/prism-player-mobile

# Expo Go (iOS/Android físico ou simulador)
npx expo start

# Android (emulador ou dispositivo)
npx expo run:android

# iOS (apenas macOS)
npx expo run:ios
```

Ao abrir o app, escanear o QR code exibido no terminal com o Expo Go ou conectar
diretamente no emulador.

No campo de URL do player inserir:

```
http://<IP-LOCAL>:8080/<stream_id>/master.m3u8
```

> Em ambiente local usar o IP da máquina host (ex: `192.168.1.100`), não
> `localhost`, pois o dispositivo físico ou emulador não resolve `localhost` para
> o host.

---

## Referência rápida — ordem dos terminais

| # | Diretório | Comando |
|---|-----------|---------|
| 1 | `openstream/` | `cargo run -p prism-node -- --config <seed.toml>` |
| 2 | `openstream/crates/prism-studio` | `PRISM_SEED_ADDRS=127.0.0.1:4002 npx tauri dev` |
| 3 | *(OBS ou ffmpeg)* | envia RTMP para `rtmp://localhost/live/test` |
| 4 | `openstream/crates/prism-player` | `npm run dev` |
| 5 | `openstream/crates/prism-player-mobile` | `npx expo start` *(opcional)* |

---

## Painéis do Studio

| Painel | Função |
|--------|--------|
| **ChatPanel** | Chat ao vivo; moderação em tempo real |
| **ModerationMenu** | Clique em usuário → bloquear / remover do stream |
| **NodeTree** | Visualiza a árvore de relay (nós A/B/C/edge) |
| **EarningsPanel** | State channels: ganhos acumulados na sessão |
| **OBSGuide** | Instruções passo a passo para configurar o OBS |

---

## Solução de problemas

### `RTMP server error: address already in use`

Porta 1935 ocupada. Verificar se outro processo usa a porta:

```bash
# Linux
ss -tlnp | grep 1935

# Windows
netstat -ano | findstr 1935
```

### `No manifest for stream` (404 no player)

- Confirmar que o seed node está rodando e mostra `chunk receiver listening`
- Confirmar que `PRISM_SEED_ADDRS` aponta para `127.0.0.1:4002`
- Studio deve estar no modo Tauri (Opção B), não dev browser (Opção A)
- OBS/ffmpeg precisa estar enviando vídeo antes de tentar assistir

### Onboarding trava no passo 3 (Benchmark)

O benchmark mede upload abrindo uma conexão ao seed node. Confirmar que o seed
node está rodando antes de iniciar o Studio.

### Chat não aparece no Player

- Confirmar que o seed node está rodando (gossipsub é iniciado pelo nó)
- O viewer precisa ter uma identidade criada na aba **Identity** antes de enviar mensagens
- Mensagens de outros viewers aparecem sem identidade configurada (somente leitura)

### `cargo run` falha com erro de linker no Windows

Instalar o **Microsoft C++ Build Tools**:
```
https://visualstudio.microsoft.com/visual-cpp-build-tools/
```
Selecionar: "Desktop development with C++"

### `npx tauri dev` falha com `webkit2gtk` não encontrado (Linux)

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev
```

### Expo: `Network response timed out` no player mobile

O dispositivo não consegue alcançar o seed node. Usar o IP da máquina host no
lugar de `localhost`:

```bash
# Linux: descobrir IP
ip route get 1 | awk '{print $7; exit}'

# Windows
ipconfig | findstr "IPv4"
```

Substituir `localhost` pela saída do comando na URL do player mobile.

### Primeiro chunk demora ~3 segundos

Comportamento esperado — o Studio acumula frames em janelas de 3 segundos antes
de enviar o primeiro chunk ao seed node.
