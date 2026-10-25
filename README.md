<p align="center">
  <img src="apps/desktop/src-tauri/icons/app-icon.svg" width="96" height="96" alt="">
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/solune-logotipo-on-dark.svg">
    <img src="docs/assets/solune-logotipo-on-light.svg" width="169" height="48" alt="solune">
  </picture>
</p>

<p align="center">Vídeos e imagens como papel de parede, com pouco uso de CPU e GPU.</p>

<!-- As badges de Rust e Tauri são fixas: atualize junto com rust-toolchain.toml e apps/desktop/src-tauri/Cargo.toml. -->
<p align="center">
  <a href="https://github.com/fabiodejesusfferreira/solune/actions/workflows/ci.yml"><img src="https://github.com/fabiodejesusfferreira/solune/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/licen%C3%A7a-MIT-123c46?labelColor=27292e" alt="Licença MIT"></a>
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/Rust-1.99.1-123c46?labelColor=27292e" alt="Rust 1.99.1"></a>
  <a href="apps/desktop/src-tauri/Cargo.toml"><img src="https://img.shields.io/badge/Tauri-2.12-123c46?labelColor=27292e" alt="Tauri 2.12"></a>
  <a href="#rodar-o-daemon-sozinho"><img src="https://img.shields.io/badge/Linux-X11_e_Wayland-123c46?labelColor=27292e" alt="Linux: X11 e Wayland"></a>
  <a href="#rodar-o-daemon-sozinho"><img src="https://img.shields.io/badge/Windows-em_desenvolvimento-123c46?labelColor=27292e" alt="Windows: em desenvolvimento"></a>
</p>

Solune é um app de desktop de código aberto que põe vídeos e imagens como papel de parede, com carrossel e troca automática entre os temas claro e escuro. O foco é o vídeo, tocando com pouco uso de CPU e GPU.

O projeto está no começo. Hoje funciona assim:

- No Linux, o daemon toca um vídeo como papel de parede e pausa sozinho com um app em tela cheia, na bateria ou com a tela bloqueada.
- O app tem duas telas, Biblioteca e Regras e desempenho, e controla o daemon.
- No Windows, o código passa no clippy do CI, mas ainda não foi compilado por inteiro e o papel de parede não aparece.

Contexto, decisões e pesquisa estão em [`docs/Solune.md`](docs/Solune.md).

## Como o Solune se divide

O Solune tem duas partes. O daemon (`solune-daemon`) toca o vídeo com o libmpv, sem webview. O app (`solune`) é só um controle remoto: fechar a janela não para o vídeo.

| Caminho | O que é |
| --- | --- |
| `apps/desktop/` | App Tauri: interface React (`src/`) e lado nativo com a bandeja e a conexão com o daemon (`src-tauri/`) |
| `crates/solune-ipc/` | Protocolo entre o app e o daemon: as mensagens e o socket local |
| `crates/solune-core/` | Lógica que não usa a GPU: regras de pausa e agenda do tema claro e escuro |
| `crates/solune-daemon/` | Daemon: libmpv, janela de papel de parede e servidor do socket |
| `packages/tokens/` | Tokens do design system (`tokens.json`), que geram CSS e TS |
| `packages/ui/` | Componentes React do design system |
| `packages/i18n/` | Textos em pt-BR e checagens de tradução |
| `scripts/` | Checagens do repositório e scripts de build |
| `docs/` | Contexto do projeto, memória, medições e decisões (`docs/decisions/`) |

As escolhas da estrutura estão em [`docs/decisions/0003-escolhas-do-scaffold.md`](docs/decisions/0003-escolhas-do-scaffold.md).

## Preparar o ambiente

- Rust: a versão de `rust-toolchain.toml`. O rustup instala sozinho.
- Node.js 24 (`.node-version`) e pnpm 12. O `corepack enable` instala a versão pedida no `package.json`.
- libmpv com API de cliente 2.x, por link dinâmico.
  - Linux: basta o pacote de runtime (`sudo apt install libmpv2` no Debian, Ubuntu e Mint). O build usa o `libmpv.so.2` direto, sem o pacote -dev.
  - Windows: um build LGPL do libmpv. Aponte `MPV_LIB_DIR` para a pasta da biblioteca de importação e deixe a DLL no `PATH`. Ainda não testado.

No Linux, o app Tauri também precisa dos pacotes da webview e da bandeja. No Debian, Ubuntu e Mint:

```sh
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
```

## Rodar o app

```sh
pnpm bootstrap   # instala as dependências JS e gera os tokens
pnpm dev         # compila o daemon e abre o app, recarregando a interface a cada mudança
```

Ao abrir, o app procura o daemon. Se ele não estiver rodando, o app inicia o `solune-daemon` da mesma pasta do executável (a variável `SOLUNE_DAEMON` aponta outro). Fechar a janela deixa o app na bandeja, com o vídeo tocando. Sair pela bandeja encerra o app e o daemon, e o papel de parede do sistema volta.

A tela Regras e desempenho mostra o uso do daemon (CPU, GPU, memória, memória de vídeo e quadros por segundo) e liga ou desliga as regras de pausa e o início com o sistema. Quando inicia com o sistema, o app abre só na bandeja (argumento `--hidden`).

Outros scripts:

| Script | O que faz |
| --- | --- |
| `pnpm dev:ui` | Abre só a interface no navegador, sem o Tauri. Os comandos para o daemon falham |
| `pnpm dev:daemon` | Roda só o daemon, o mesmo que `cargo daemon` |
| `pnpm build` | Gera o instalador: `.deb` no Linux e NSIS no Windows, em `target/release/bundle` |
| `pnpm ipc:fixtures` | Regrava os exemplos do protocolo depois de uma mudança feita de propósito |

A biblioteca ainda é provisória: guarda só o caminho dos vídeos, no armazenamento da webview, sem miniatura.

## Gerar os instaladores

`pnpm build` gera o `.deb` no Linux e o instalador NSIS (`.exe`) no Windows. Os comandos para rpm, AppImage, MSI e Flatpak, o que cada sistema consegue gerar e onde o daemon e o libmpv entram em cada pacote estão em [`docs/empacotamento.md`](docs/empacotamento.md).

## Rodar o daemon sozinho

Num terminal, inicie o daemon:

```sh
cargo daemon [--hwdec MODO] [--headless]
```

Em outro, mande os comandos:

```sh
cargo daemon play caminho/do/video.mp4
cargo daemon pause      # e resume
cargo daemon status     # estado, arquivo e decodificação em uso
cargo daemon stop       # volta o papel de parede do sistema
cargo daemon policy     # regras de pausa e o que o daemon detecta agora
cargo daemon usage      # CPU, GPU, memória e quadros do daemon
cargo daemon set pause_on_battery off   # liga ou desliga uma regra
cargo daemon quit
```

O daemon escolhe onde desenhar pela sessão gráfica:

| Sessão | Onde o vídeo aparece | Status |
| --- | --- | --- |
| Wayland (`WAYLAND_DISPLAY`) | Camada de fundo do layer-shell: KDE Plasma 6 e wlroots (Sway, Hyprland) | Testado só num Sway 1.9 sem tela, com renderização por software |
| X11 (`DISPLAY`) | Janela do tipo área de trabalho, como no Cinnamon do Linux Mint | Testado no Linux Mint com Cinnamon: ícones da área de trabalho por cima, VA-API em uso |
| Windows | WorkerW | Ainda não implementado: `play` responde `surface_unavailable` |

`--hwdec` repassa o modo de decodificação ao mpv (padrão `auto-safe`; `no` desliga a aceleração). `--headless` toca sem tela (`vo=null`), para testes. O socket fica em `$XDG_RUNTIME_DIR/solune/daemon.sock`, e `SOLUNE_SOCKET` troca o caminho.

No Linux, o libwayland, o libX11 e o libEGL são carregados quando o daemon roda. Por isso o daemon compila sem nenhum pacote -dev, só com o `libmpv2`.

### Pausa automática

O daemon pausa sozinho quando ninguém vê o papel de parede. Cada regra pode ser desligada no app ou com `cargo daemon set`. As regras são `pause_on_fullscreen`, `pause_on_battery`, `pause_on_lock` e `release_when_paused`. Pausado na bateria, a tela continua com a última imagem do vídeo.

| Sinal | Como o daemon detecta | Status |
| --- | --- | --- |
| App em tela cheia (X11) | `_NET_WM_STATE_FULLSCREEN` pelo EWMH, na área de trabalho atual | Testado no Linux Mint com Cinnamon e em Xvfb com openbox |
| App em tela cheia (Wayland) | wlr-foreign-toplevel-management: Sway, Hyprland | Testado num Sway sem tela. O KDE Plasma não tem esse protocolo e fica sem detecção |
| Bateria | `/sys/class/power_supply` a cada 10 s | Testado só com arquivos simulados |
| Tela bloqueada | D-Bus: protetor de tela do Cinnamon, GNOME, MATE e KDE, e o `LockedHint` do logind | Testado só com um sinal simulado num barramento de teste |
| Windows | — | Ainda não: as regras ficam sem detecção |

Depois de 5 minutos pausado (`release_when_paused`), o daemon libera o decodificador: guarda o quadro atual na GPU, fecha o arquivo e desliga o render do libmpv, o que solta o decodificador e a memória de vídeo que ele usava. A tela continua com o mesmo quadro. Ao retomar, o daemon religa o render e abre o vídeo na mesma posição. `SOLUNE_RELEASE_AFTER_SECS` troca os 5 minutos, para testes e medições.

As regras e o último papel de parede ficam em `~/.config/solune` (`settings.json` e `state.json`). No Windows, em `%APPDATA%\Solune`. `SOLUNE_CONFIG_DIR` troca a pasta. Ao iniciar, o daemon volta a tocar o último papel de parede. As decisões estão em [`docs/decisions/0004-politica-de-pausa.md`](docs/decisions/0004-politica-de-pausa.md).

Limitações conhecidas:

- No wlroots, outro programa de papel de parede na mesma camada (como o `swaybg` ou o `hyprpaper`) pode ficar por cima do vídeo. Feche-o enquanto testa.
- Com renderização por software (llvmpipe), o mpv às vezes desenha quadros pretos ou pela metade durante uma execução inteira. Acontece também com o mpv de linha de comando (`--vo=gpu`), então não vem do daemon. Rode de novo.

## Verificar o código

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
SOLUNE_TEST_VIDEO=video-curto.mp4 dbus-run-session -- cargo test -p solune-daemon -- --ignored
cargo deny check
pnpm install && pnpm tokens
pnpm lint && pnpm typecheck && pnpm test
```

## Licença

[MIT](LICENSE). O libmpv é LGPL e entra só por link dinâmico. Os bindings do libmpv foram escritos a partir dos cabeçalhos do mpv, que têm licença ISC. Veja [`docs/decisions/0002-licenca-mit-e-libmpv.md`](docs/decisions/0002-licenca-mit-e-libmpv.md).

O logotipo usa a fonte Outfit (OFL), convertida em curvas.
