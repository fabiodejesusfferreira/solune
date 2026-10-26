# 0003. Escolhas do scaffold completo

Data: 25 out. 2026. Status: aceita.

## Contexto

O plano do setup inicial deixou quatro decisões para o scaffold completo, cada uma com uma recomendação. O usuário pediu para seguir as recomendações.

## Decisão

- **Bandeja.** Quem mostra o ícone da bandeja é o app Tauri, residente sem janela. O daemon continua sem interface e sem texto.
- **Fonte de verdade dos tokens.** O arquivo `packages/tokens/tokens.json` do repositório. O artefato do design system vira a vitrine e é atualizado a partir dele.
- **Lint e format do JS.** Biome, uma ferramenta só para TS, JSON e CSS, sem depender do compilador do TypeScript.
- **i18n.** i18next com react-i18next e catálogos JSON, que o lado Rust do app também consegue ler.
- **Tipos entre Rust e TypeScript.** Escritos à mão em `apps/desktop/src/daemon/protocol.ts`, conferidos por um teste de contrato. O plano previa gerar os tipos com o tauri-specta, mas a linha estável dele (1.0.2) só funciona com o Tauri 1, e a do Tauri 2 ainda é release candidate (2.0.0-rc.26). Correção feita no scaffold, em 25 out. 2026.

## Consequências

- Mudança de cor, espaço ou tipografia começa no `tokens.json`, passa pela checagem de contraste no CI e só depois vai para o artefato.
- O daemon responde com códigos (`pause_reason`, `error`), e quem traduz é a UI ou a bandeja. Há um catálogo só, em `packages/i18n`.
- O protocolo tem exemplos em JSON gravados pelo Rust (`crates/solune-ipc/fixtures`). O teste do Rust quebra se a serialização mudar, e o do Vitest quebra se os tipos TS não aceitarem os exemplos ou se a lista de códigos for diferente. Mudança de propósito: `pnpm ipc:fixtures` e ajuste do `protocol.ts`. Vale trocar pelo tauri-specta quando ele sair estável para o Tauri 2.
- A assinatura de código no Windows continua em aberto, para decidir antes do primeiro release público.
