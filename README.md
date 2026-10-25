# Solune

Solune é um app de desktop de código aberto que põe vídeos e imagens como papel de parede, com carrossel e troca automática entre os temas claro e escuro. O carro-chefe é o vídeo, tocando com baixo uso de CPU e GPU.

O projeto está no começo: o daemon já toca um vídeo como papel de parede no Linux, e o app tem a primeira tela, a biblioteca, falando com o daemon. Contexto, decisões e pesquisa estão em [`docs/Solune.md`](docs/Solune.md).

## Estrutura

| Caminho | O que é |
| --- | --- |
| `apps/desktop/` | App Tauri: interface React (`src/`) e lado nativo com bandeja e ponte com o daemon (`src-tauri/`) |
| `crates/solune-ipc/` | Protocolo entre o app e o daemon: mensagens, framing e socket local |
| `crates/solune-core/` | Lógica sem GPU: política de pausa e agenda de tema claro e escuro |
| `crates/solune-daemon/` | Daemon: libmpv, superfícies de papel de parede e servidor do socket |
| `packages/tokens/` | Tokens do design system (`tokens.json`), que geram CSS e TS |
| `packages/ui/` | Componentes React do design system |
| `packages/i18n/` | Catálogo pt-BR e checagens de tradução |
| `scripts/` | Checagens do repositório e scripts de build |
| `docs/` | Contexto do projeto, memória, medições e decisões (`docs/decisions/`) |

As escolhas do scaffold estão em [`docs/decisions/0003-escolhas-do-scaffold.md`](docs/decisions/0003-escolhas-do-scaffold.md).

## Requisitos

- Rust: a versão fixada em `rust-toolchain.toml` (o rustup instala sozinho).
- Node.js 24 (`.node-version`) e pnpm 12 (`corepack enable` instala a versão do `package.json`).
- libmpv com API de cliente 2.x, linkado dinamicamente.
  - Linux: o pacote de runtime basta (`sudo apt install libmpv2` no Debian, Ubuntu e Mint). O build linka direto com o `libmpv.so.2`, sem o -dev.
  - Windows: um build LGPL do libmpv; aponte `MPV_LIB_DIR` para a pasta da biblioteca de importação e deixe a DLL no `PATH`. Ainda não testado.

No Linux, o app Tauri também precisa dos pacotes da webview e da bandeja. No Debian, Ubuntu e Mint:

```sh
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

## Rodar o app

```sh
pnpm bootstrap   # instala as dependências JS e gera os tokens
pnpm dev         # compila o daemon e abre o app com recarga da interface
```

Ao abrir, o app procura o daemon e, se ele não estiver rodando, inicia o `solune-daemon` que está na mesma pasta do executável (`SOLUNE_DAEMON` aponta outro). Fechar a janela deixa o app na bandeja. Sair pela bandeja fecha só o app: o daemon continua com o papel de parede.

Outros scripts:

| Script | O que faz |
| --- | --- |
| `pnpm dev:ui` | Só a interface no navegador, sem o Tauri (os comandos do daemon falham) |
| `pnpm dev:daemon` | Só o daemon, o mesmo que `cargo daemon` |
| `pnpm build` | Daemon em release, app e instalador (`.deb` no Linux, NSIS no Windows), em `target/release/bundle` |
| `pnpm ipc:fixtures` | Regrava os exemplos do protocolo depois de uma mudança de propósito |

A biblioteca ainda é provisória: guarda só o caminho dos vídeos no armazenamento da webview, sem miniatura.

## Rodar o daemon sozinho

Num terminal, o daemon:

```sh
cargo daemon [--hwdec MODO] [--headless]
```

Em outro, os comandos para ele:

```sh
cargo daemon play caminho/do/video.mp4
cargo daemon pause      # e resume
cargo daemon status     # estado, arquivo e decodificação em uso
cargo daemon stop       # volta o papel de parede do sistema
cargo daemon quit
```

O daemon escolhe a superfície pela sessão:

| Sessão | Superfície | Status |
| --- | --- | --- |
| Wayland (`WAYLAND_DISPLAY`) | Camada de fundo do layer-shell: KDE Plasma 6 e wlroots (Sway, Hyprland) | Testado só num Sway 1.9 sem tela, com renderização por software |
| X11 (`DISPLAY`) | Janela do tipo desktop, como no Cinnamon do Linux Mint | Testado no Linux Mint com Cinnamon: ícones da área de trabalho por cima, VA-API em uso |
| Windows | WorkerW | Ainda não implementado: `play` responde `surface_unavailable` |

`--hwdec` repassa o modo ao mpv (padrão `auto-safe`; `no` desliga). `--headless` toca sem tela (`vo=null`), para testes. O socket fica em `$XDG_RUNTIME_DIR/solune/daemon.sock`; `SOLUNE_SOCKET` troca o caminho.

No Linux, o libwayland, o libX11 e o libEGL são carregados em tempo de execução, então o build não precisa de pacotes -dev além do libmpv.

Limitações conhecidas:

- No wlroots, um outro programa de papel de parede na mesma camada (como o `swaybg` ou o `hyprpaper`) pode ficar por cima do vídeo. Feche-o enquanto testa.
- Com renderização por software (llvmpipe), o mpv às vezes desenha quadros pretos ou pela metade durante uma execução inteira. Acontece também com o mpv de linha de comando (`--vo=gpu`), então não é do daemon; rode de novo.

## Verificações

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
SOLUNE_TEST_VIDEO=video-curto.mp4 cargo test -p solune-daemon -- --ignored
cargo deny check
pnpm install && pnpm tokens
pnpm lint && pnpm typecheck && pnpm test
```

## Licença

[MIT](LICENSE). O libmpv é LGPL e entra só por link dinâmico; os bindings do libmpv foram escritos a partir dos cabeçalhos do mpv, que têm licença ISC. Veja [`docs/decisions/0002-licenca-mit-e-libmpv.md`](docs/decisions/0002-licenca-mit-e-libmpv.md).
