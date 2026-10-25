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
  - Linux: o `libmpv.so` da distribuição (vem com `libmpv-dev` no Debian e Ubuntu). Sem o pacote -dev, crie um link `libmpv.so` para o `libmpv.so.2` numa pasta e aponte `MPV_LIB_DIR` para ela.
  - Windows: um build LGPL do libmpv; aponte `MPV_LIB_DIR` para a pasta da biblioteca de importação e deixe a DLL no `PATH`. Ainda não testado.

## Rodar o spike

```sh
cargo spike caminho/do/video.mp4 [--surface janela|layer-shell] [--seconds N] [--hwdec MODO] [--vo SAIDA]
```

| Superfície | Onde o vídeo aparece | Status |
| --- | --- | --- |
| `janela` (padrão) | Janela do próprio mpv | Referência; `--vo null` toca sem tela (teste de fumaça) |
| `layer-shell` | Camada de fundo do Wayland: KDE Plasma 6 e wlroots (Sway, Hyprland) | Testado só num Sway 1.9 sem tela, com renderização por software |

O spike imprime o decodificador em uso (`hwdec`), a saída de vídeo e o formato do arquivo. `--hwdec` repassa o modo ao mpv (padrão `auto-safe`; `no` desliga).

No Linux, o modo `layer-shell` carrega o libwayland e o libEGL em tempo de execução, então o build não precisa de pacotes -dev além do libmpv.

## Verificações

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
```

## Licença

[MIT](LICENSE). O libmpv é LGPL e entra só por link dinâmico; os bindings do spike foram escritos a partir dos cabeçalhos do mpv, que têm licença ISC. Veja [`docs/decisions/0002-licenca-mit-e-libmpv.md`](docs/decisions/0002-licenca-mit-e-libmpv.md).
