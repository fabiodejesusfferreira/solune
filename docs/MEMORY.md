# Solune: memória do projeto

Fatos estáveis do projeto, para retomar o trabalho sem perguntar de novo. Fontes: `docs/Solune.md` e o design system [Solune](https://claude.ai/artifact/6JxrNV7jBX57aXoqcUqtd2) (versão de 25 out. 2026). Atualizado em 25 out. 2026.

## Produto

- Solune é um app de desktop de código aberto que põe vídeos e imagens como papel de parede, com carrossel e troca automática entre claro e escuro. O carro-chefe é o vídeo, com baixo uso de CPU e GPU.
- Nome: sol + lune. No texto corrido, "Solune"; no logo, "solune" em minúsculas com o "l" em âmbar (`sun`).
- MVP (v0.1): Windows 10/11 e Linux (KDE Plasma 6 e wlroots). GNOME na v0.2, por extensão. macOS por último.

## Repositório e licença

- Repositório: https://github.com/fabiodejesusfferreira/solune.
- Licença MIT. O libmpv entra só por link dinâmico e em build LGPL; bindings escritos à mão a partir dos cabeçalhos ISC do mpv, sem o crate `libmpv2` (LGPL). Ver `docs/decisions/0002-licenca-mit-e-libmpv.md`.
- Setup em duas etapas: fase 0 (setup mínimo, só o spike no workspace) e scaffold completo depois do spike. Plano completo no doc [Solune: plano do setup inicial do app](https://claude.ai/artifact/W5c5qydHRWsGpXjXNP24Cf).

## Arquitetura

- UI em Tauri v2 com React. É só um controle remoto: o wallpaper continua tocando com a janela fechada.
- Player em daemon Rust separado, com libmpv pela render API, sem webview. UI e daemon conversam por socket local.
- Superfícies: WorkerW no Windows, wlr-layer-shell no KDE 6 e wlroots, extensão do GNOME Shell no GNOME, janela desktop no X11.
- Política de desempenho: pausa em tela cheia, bateria e tela bloqueada; destrói o contexto de decodificação após 5 minutos pausado; converte vídeos na importação para 1080p a 30 fps.

## Design system

Fonte de verdade: o artefato [Solune](https://claude.ai/artifact/6JxrNV7jBX57aXoqcUqtd2). Na interface, use sempre `var(--nome)` dos tokens, nunca hexadecimal solto. Tema por `data-theme="light"` ou `data-theme="dark"` no elemento raiz; o padrão segue o sistema.

### Cores (claro / escuro)

| Token | Claro | Escuro | Uso |
| --- | --- | --- | --- |
| surface-0 | #f5f3ee | #16171a | Fundo da janela |
| surface-1 | #fbfaf7 | #1d1e22 | Painéis e cartões |
| surface-2 | #ece8df | #27292e | Hover, selecionado, trilha desligada |
| ink | #123c46 | #ecedef | Texto principal e ícones |
| ink-muted | #4b646b | #a3a7ae | Texto secundário e legendas |
| line | #dad5c8 | #2e3036 | Divisores decorativos (nunca o único limite de um controle) |
| line-strong | #6f8286 | #80858e | Borda de controles, 3:1 ou mais |
| primary | #123c46 | #ecedef | Botão primário e interruptor ligado |
| on-primary | #f5f3ee | #16171a | Texto sobre primary |
| sun | #f7a23b | #f7a23b | Único destaque; no máximo um elemento cheio por tela |
| on-sun | #1a1205 | #1a1205 | Texto sobre sun |
| sun-text | #8a5300 | #f7a23b | Texto na cor do sol (nunca `sun` como texto no claro) |
| focus | #123c46 | #f7a23b | Anel de foco de 2px com 2px de afastamento |
| success | #1b6b43 | #5fd08f | Estado concluído, sempre com ícone e palavra |
| danger | #a8261d | #ff8f85 | Erro e ação destrutiva, sempre com ícone e frase |
| scrim | rgba(0,0,0,0.62) | rgba(0,0,0,0.72) | Véu sobre miniaturas para texto legível |

O escuro é grafite, nunca preto puro.

### Tipografia

- Outfit nos títulos: `display` 40/44 500, `title` 24/30 500, `heading` 18/24 500.
- Figtree no texto: `body` 14/20 400, `body-strong` 14/20 500, `label` 12/16 500, `caption` 12/16 400.
- JetBrains Mono nos números: `data` 12/16 400, algarismos tabulares.
- Todas OFL. Empacotar os woff2 localmente no Tauri: o app precisa funcionar sem rede. Os arquivos de fonte ainda não estão no design system.

### Espaço, forma, elevação e movimento

- Espaço em base 4px: `space-1` 4, `space-2` 8, `space-3` 12, `space-4` 16, `space-5` 24, `space-6` 32, `space-7` 48. Painéis com `space-4` de padding; cartões da grade a `space-5`.
- Raios: `radius-sm` 6px (chips, campos), `radius-md` 10px (botões, cartões), `radius-lg` 16px (painéis, diálogos), `radius-pill` 999px (interruptores).
- Sombras: `shadow-raised` em cartões, `shadow-popover` em menus e dicas. Prefira borda e troca de superfície; no máximo dois níveis flutuantes.
- Movimento: 120ms para cor, 160ms para o interruptor, ease-out. Único movimento decorativo: o sol em órbita pequena durante carregamentos. Respeitar `prefers-reduced-motion`.
- Densidade: botões de 36px (28px compacto), linhas de lista de 40px, alvos de pelo menos 28px.

### Ícones, logo e componentes

- Ícones Lucide (ISC) via `lucide-react`, traço de 1,5px, grade de 16 e 20px, herdando `ink` ou `ink-muted`. Sem emoji como ícone.
- Logo: marca completa (lua crescente com o sol em órbita inclinada) a partir de 32px; abaixo disso, o símbolo sem órbita. Quatro SVGs: `solune-mark-on-light`, `solune-mark-on-dark`, `solune-symbol-on-light`, `solune-symbol-on-dark`. Escolher o arquivo pelo fundo, nunca recolorir por CSS. Área livre de meio diâmetro da lua.
- Componentes React do design system: `Button`, `Toggle`, `Chip`, `WallpaperCard`.

### Conteúdo e acessibilidade

- Tom calmo e preciso, sentence case, verbo no início, sem exclamação. Português do Brasil como base, com 30% de folga para inglês, espanhol e alemão.
- Erros dizem o que aconteceu e o que fazer, em uma frase. Evitar "simplesmente", "fácil", "incrível" e "com sucesso".
- Texto com 4,5:1 ou mais nos dois temas; bordas e foco com 3:1 ou mais. Estado nunca só pela cor. Tudo operável por teclado, na ordem visual.

## UI e UX

Sete telas aprovadas no canvas [Solune UI e UX](https://claude.ai/artifact/T65bCcwBpccbTF9PXA5DgT): Biblioteca, Detalhe e aplicar, Monitores e carrossel, Claro e escuro, Regras e desempenho, Primeiros passos e Menu da bandeja. Os números do canvas são exemplos, não medições.

## Pontos em aberto

- Comportamento do WorkerW no Windows 11 24H2.
- Suporte ao GNOME por extensão.
- Aceleração por hardware com NVIDIA em Wayland.
- Busca de marca no INPI, EUIPO e USPTO.
- Wordmark ainda não convertido em curvas.
- Lua dos SVGs escuros em #e4f1ef (tom azulado), diferente do `ink` escuro (#ecedef).
- Telas de configurações gerais e de importação com progresso de conversão.

## Próximo passo

Spike do player em `spikes/player/`: a etapa 1 (libmpv tocando um mp4 na janela do mpv) compila e passou no teste de fumaça sem tela; ainda não foi vista numa tela real. Faltam o WorkerW no Windows 11 24H2 e o layer-shell no KDE ou Sway, com CPU e GPU medidos. Depois, o scaffold completo do workspace.

## Regras de trabalho

Seja direto, diga o que não verificou, não invente números nem medições, e use os tokens do design system em vez de cores soltas.
