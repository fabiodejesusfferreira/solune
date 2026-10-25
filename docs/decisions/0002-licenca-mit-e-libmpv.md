# 0002. Licença MIT e libmpv por link dinâmico

Data: 25 out. 2026. Status: aceita.

## Contexto

O repositório foi criado com licença MIT. O libmpv é GPL ou LGPL, conforme a compilação, e o crate `libmpv2` é LGPL-2.1. Crates Rust são linkados estaticamente, o que complicaria cumprir a LGPL dentro de um binário MIT.

## Decisão

- O projeto fica MIT.
- O libmpv entra só por link dinâmico, e só em builds LGPL. Builds GPL do mpv ou do FFmpeg ficam de fora.
- Os bindings do libmpv são escritos à mão a partir de `mpv/client.h` e `mpv/render.h`, que têm licença ISC. Nenhum crate copyleft entra no grafo.
- O `cargo-deny` barra licenças fora da lista do `deny.toml`.

## Consequências

- No Windows, o build do libmpv distribuído precisa ser LGPL. Ainda falta escolher de onde ele vem.
- A conversão na importação (1080p a 30 fps) não pode usar um encoder GPL como o x264. Opções compatíveis ficam para a fase de importação.
- Distribuir o Solune com o libmpv LGPL exige mandar junto o aviso de licença do libmpv e permitir trocar a DLL ou a `.so`.
