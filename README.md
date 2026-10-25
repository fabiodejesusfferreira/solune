# Solune

Solune é um app de desktop de código aberto que põe vídeos e imagens como papel de parede, com carrossel e troca automática entre os temas claro e escuro. O carro-chefe é o vídeo, tocando com baixo uso de CPU e GPU.

O projeto está no começo: este repositório tem só o setup mínimo e o spike do player. Contexto, decisões e pesquisa estão em [`docs/Solune.md`](docs/Solune.md).

## Estrutura

| Caminho | O que é |
| --- | --- |
| `spikes/player/` | Spike do player: libmpv tocando um vídeo, na janela do mpv ou como papel de parede. Fora do release |
| `docs/` | Contexto do projeto, memória e decisões (`docs/decisions/`) |
| `.github/workflows/ci.yml` | CI no Linux e no Windows |

O layout completo (daemon, protocolo de IPC, app Tauri, tokens, i18n) entra depois do spike. Veja [`docs/decisions/0001-setup-em-duas-etapas.md`](docs/decisions/0001-setup-em-duas-etapas.md).

## Requisitos

- Rust: a versão fixada em `rust-toolchain.toml` (o rustup instala sozinho).
- libmpv com API de cliente 2.x, linkado dinamicamente.
  - Linux: o pacote de runtime basta (`sudo apt install libmpv2` no Debian, Ubuntu e Mint). O build linka direto com o `libmpv.so.2`, sem o -dev.
  - Windows: um build LGPL do libmpv; aponte `MPV_LIB_DIR` para a pasta da biblioteca de importação e deixe a DLL no `PATH`. Ainda não testado.

## Rodar o spike

```sh
cargo spike caminho/do/video.mp4 [--surface janela|layer-shell|x11] [--seconds N] [--hwdec MODO] [--vo SAIDA]
```

| Superfície | Onde o vídeo aparece | Status |
| --- | --- | --- |
| `janela` (padrão) | Janela do próprio mpv | Referência; `--vo null` toca sem tela (teste de fumaça) |
| `layer-shell` | Camada de fundo do Wayland: KDE Plasma 6 e wlroots (Sway, Hyprland) | Testado só num Sway 1.9 sem tela, com renderização por software |
| `x11` | Janela do tipo desktop em sessões X11, como o Cinnamon do Linux Mint | Testado só num Xvfb sem gerenciador de janelas, com renderização por software |

O spike imprime o decodificador em uso (`hwdec`), a saída de vídeo e o formato do arquivo. `--hwdec` repassa o modo ao mpv (padrão `auto-safe`; `no` desliga).

No Linux, os modos `layer-shell` e `x11` carregam o libwayland, o libX11 e o libEGL em tempo de execução, então o build não precisa de pacotes -dev além do libmpv.

Com renderização por software (llvmpipe), o mpv às vezes desenha quadros pretos ou pela metade durante uma execução inteira. Acontece também com o mpv de linha de comando (`--vo=gpu`), então não é do spike; rode de novo. Em GPU real ainda não foi testado.

## Verificações

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
```

## Licença

[MIT](LICENSE). O libmpv é LGPL e entra só por link dinâmico; os bindings do spike foram escritos a partir dos cabeçalhos do mpv, que têm licença ISC. Veja [`docs/decisions/0002-licenca-mit-e-libmpv.md`](docs/decisions/0002-licenca-mit-e-libmpv.md).
