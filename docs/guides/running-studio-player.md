# Rodando Studio + Player — Guia Local

Este guia cobre o setup completo em **Windows** e **Linux** para uma sessão de
streaming local: seed node → Studio (streamer) → Player (viewer).

---

## Pré-requisitos

### Rust + Cargo

```bash
# Windows e Linux — instalar via rustup
curl https://sh.rustup.rs -sSf | sh   # Linux/macOS
# Windows: baixar rustup-init.exe em https://rustup.rs

rustup default stable
rustup update
```

### Node.js ≥ 18

```bash
# Linux (via nvm recomendado)
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
nvm install 20

# Windows — baixar instalador em https://nodejs.org
# Ou via winget:
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
- Instalar **WebView2** (já incluso no Windows 10/11 atualizado)

---

## Estrutura de portas

| Porta | Serviço | Quem abre |
|-------|---------|-----------|
| `1935` | RTMP — OBS/ffmpeg envia vídeo aqui | Studio (Rust) |
| `4001` | P2P libp2p (DHT/QUIC) | prism-node |
| `4002` | Recepção de chunks TCP | prism-node |
| `5173` | Dev server do Studio (Vite) | npm |
| `5174` | Dev server do Player (Vite) | npm |
| `8080` | HLS HTTP — player busca manifests/segmentos aqui | prism-node |

---

## Passo 1 — Compilar o workspace Rust

```bash
cd openstream   # raiz do repositório

cargo build -p prism-node
```

A primeira compilação demora alguns minutos (libp2p, axum, ed25519-dalek).

---

## Passo 2 — Criar a config do seed node

### Linux

```bash
mkdir -p /tmp/prism

cat > /tmp/prism/seed.toml <<EOF
key_path        = "/tmp/prism/seed.key"
listen_addr     = "/ip4/0.0.0.0/tcp/4001"
bootstrap_peers = []
capacity_class  = "A"
EOF
```

### Windows (PowerShell)

```powershell
New-Item -ItemType Directory -Force -Path "$env:TEMP\prism"

@"
key_path        = "$prismTemp/seed.key"
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

Você deve ver logs como:
```
INFO prism_node: identity ready
INFO prism_node: edge HTTP server listening addr=0.0.0.0:8080
INFO prism_node: chunk receiver listening addr=0.0.0.0:4002
INFO prism_node: prism-node running
```

---

## Passo 4 — Instalar dependências npm

### Studio

```bash
cd openstream/crates/prism-studio
npm install
```

### Player

```bash
cd openstream/crates/prism-player
npm install
```

---

## Passo 5 — Iniciar o Studio

Escolha **uma** das opções abaixo:

### Opção A — Dev mode (só browser, sem app nativo)

Útil para desenvolver a UI sem precisar do Tauri instalado.

```bash
cd openstream/crates/prism-studio

# Linux
PRISM_SEED_ADDRS=127.0.0.1:4002 npm run dev

# Windows (PowerShell)
$env:PRISM_SEED_ADDRS = "127.0.0.1:4002"
npm run dev
```

Abrir no browser: **http://localhost:5173**

> **Neste modo** o botão GO LIVE mostra um `stream_id` fictício (`dev-mock-stream-id`)
> e não envia dados reais ao seed node — serve apenas para testar a UI.

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

O app abre uma janela nativa. Ao clicar **GO LIVE**:
1. A identidade Ed25519 é carregada (ou gerada em `prism_studio.key`)
2. O servidor RTMP sobe na porta `1935`
3. O `stream_id` (hash hex de 64 chars) é exibido na tela

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

### Com ffmpeg (sem OBS)

```bash
# Gera vídeo de teste sintético (barra de cor + tom de 440 Hz)
ffmpeg -re \
  -f lavfi -i testsrc=size=1280x720:rate=30 \
  -f lavfi -i sine=frequency=440:sample_rate=44100 \
  -c:v libx264 -preset veryfast -b:v 1000k \
  -c:a aac -b:a 128k \
  -f flv rtmp://localhost/live/test
```

---

## Passo 7 — Iniciar o Player

Em outro terminal:

```bash
cd openstream/crates/prism-player
npm run dev
```

Abrir no browser: **http://localhost:5174**

No campo de URL do player, inserir:

```
http://localhost:8080/<stream_id>/master.m3u8
```

Substituir `<stream_id>` pelo valor exibido no Studio após clicar GO LIVE.

Exemplo:
```
http://localhost:8080/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2/master.m3u8
```

---

## Referência rápida — ordem dos terminais

| # | Diretório | Comando |
|---|-----------|---------|
| 1 | `openstream/` | `cargo run -p prism-node -- --config <seed.toml>` |
| 2 | `openstream/crates/prism-studio` | `PRISM_SEED_ADDRS=127.0.0.1:4002 npx tauri dev` |
| 3 | *(OBS ou ffmpeg)* | envia RTMP para `rtmp://localhost/live/test` |
| 4 | `openstream/crates/prism-player` | `npm run dev` |

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
- O Studio precisa estar no modo Tauri (Opção B), não dev browser (Opção A)
- O OBS/ffmpeg precisa estar enviando vídeo antes do primeiro chunk de 3s ser gerado

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

### Primeiro chunk demora ~3 segundos

Comportamento esperado — o Studio acumula frames em janelas de 3 segundos antes de
enviar o primeiro chunk ao seed node.
