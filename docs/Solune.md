# Solune: contexto do projeto

Oct 25, 2026 · @Dejesus Dev

## Visão geral

Solune é um app de desktop de código aberto que põe vídeos e imagens como papel de parede, com carrossel e troca automática entre os temas claro e escuro. O carro-chefe é o vídeo, tocando com baixo uso de CPU e GPU.

| Item | Decisão |
| --- | --- |
| Nome | Solune: sol + lune (lua). O "l" é a letra que as duas palavras compartilham |
| Plataformas | Windows e Linux (KDE Plasma 6 e wlroots) no MVP; GNOME na v0.2; macOS por último |
| Interface | Tauri v2 com React, em uma janela que abre e fecha |
| Player | Processo Rust separado (daemon) com libmpv, sem webview |
| Licença | Código aberto; fontes OFL e ícones Lucide (ISC) |
| Público | Brasil e mercado global, em português com inglês, espanhol e alemão previstos |

O problema: quem quer vídeo como wallpaper hoje precisa de um app por sistema (Lively no Windows, Hidamari ou extensões no Linux, Backdrop no macOS) e todos têm lacunas de desempenho ou de plataforma. O Solune é um só app, leve e multiplataforma.

## Pesquisa de mercado

Veredito: existe, com espaço para diferenciar. Vídeo como wallpaper já existe em todas as plataformas; a lacuna é um app único, gratuito e multiplataforma que também gerencia imagens, carrossel e claro/escuro. Não encontrei nenhum app Tauri de wallpaper com vídeo. A pesquisa cobriu cerca de 20 buscas em inglês e português; "não encontrei" não prova que não existe.

| Concorrente | Plataforma | Ponto forte | Lacuna |
| --- | --- | --- | --- |
| [Lively](https://github.com/Lively-wallpaper/lively) | Windows | Grátis, vídeo via mpv/VLC, pausa em tela cheia | Só Windows |
| [Wallpaper Engine](https://help.wallpaperengine.io/en/functionality/linuxmacos.html) | Windows, Android | Líder, Workshop | Sem suporte oficial a Linux e macOS |
| [Hidamari](https://www.linuxlinks.com/linux-candy-hidamari-fun-video-wallpaper/) | Linux | Vídeo, site e HTML | Python com VLC; pausa em tela cheia só em X11 |
| [Hanabi](https://www.linuxuprising.com/2023/04/hanabi-is-live-wallpaper-for-gnome.html) | GNOME | Extensão com VA-API experimental | Só GNOME, instalação manual |
| [mpvpaper e wallr](https://docs.rs/crate/wallr/0.6.4) | Wayland (wlroots) | Vídeo em Hyprland, Sway, niri e KDE 6 | GNOME não suportado, sem interface gráfica |
| [ScreenPlay](https://gitlab.com/kelteseth/ScreenPlay/-/issues/158) | Windows, macOS | Código aberto, Qt6 | Linux ainda é só proposta |
| [Variety](https://linuxuprising.com/2018/09/variety-wallpaper-changer-and.html) | Linux | Carrossel e fontes online | Sem vídeo |

O que aproveitar: pausa automática em tela cheia (Lively), playlists nomeadas (Hidamari Prism), pausa em bateria (Gink) e fontes online (Variety). Reclamações recorrentes que o Solune ataca: alto consumo de CPU e GPU, projetos abandonados e GNOME sem suporte nativo.

## Arquitetura de playback

A interface e o player são separados. A UI do Solune (Tauri e React) só existe enquanto a janela está aberta; o wallpaper roda em um processo Rust nativo, sem webview, que fica em segundo plano. Se a UI fecha, o wallpaper continua tocando. Os dois conversam por um socket local.

| Camada | Papel |
| --- | --- |
| Player | libmpv pela render API, com decodificação por hardware (VA-API, NVDEC, D3D11VA, VideoToolbox) |
| Superfície | Onde o frame aparece; muda em cada plataforma (tabela abaixo) |
| Política | Decide tocar, pausar ou parar: app em tela cheia, bateria, tela bloqueada |
| Agendador | Troca de coleção por tema do sistema ou horário |
| Biblioteca | SQLite com metadados, miniaturas, pares claro/escuro e playlists |

| Plataforma | Superfície | Risco |
| --- | --- | --- |
| Windows | Janela com libmpv atrás dos ícones (WorkerW) | Alto: o Windows 11 24H2 quebrou apps de wallpaper e a mensagem 0x052C pode não criar mais o WorkerW |
| Linux, KDE 6 e wlroots | wlr-layer-shell na camada background | Médio |
| Linux, GNOME | Extensão do GNOME Shell, pois o GNOME não tem layer-shell | Alto |
| Linux, X11 | Janela do tipo desktop (\_NET\_WM\_WINDOW\_TYPE\_DESKTOP) | Baixo |
| macOS | Fora do MVP | Em aberto |

A política de desempenho é onde o argumento de leveza se ganha: além de pausar, o daemon destrói o contexto de decodificação depois de 5 minutos pausado e converte o vídeo na importação para um perfil leve (1080p a 30 fps). Pontos em aberto: aceleração por hardware com NVIDIA em Wayland e a licença do libmpv (LGPL ou GPL, conforme a compilação).

## MVP e ordem de construção

O MVP (v0.1) entrega vídeo local, imagem, carrossel por pasta, claro/escuro e pausa em tela cheia, no Windows e no Linux com KDE e wlroots. Ficam de fora: biblioteca online (Wallhaven, Unsplash), Workshop, wallpapers web, áudio reativo, widgets, tela de bloqueio e macOS.

1. Spike de risco (1 a 2 semanas): um binário Rust que toca um mp4 via libmpv atrás dos ícones no Windows 11 24H2 e em layer-shell no KDE ou Sway.
2. Daemon e IPC com play, pause, set\_wallpaper por monitor e status.
3. Política: pausa em tela cheia e bateria, medindo CPU e GPU.
4. UI em Tauri e React conversando com o daemon.
5. Tema claro/escuro e carrossel.
6. Empacotamento: MSI ou MSIX, Flatpak e AppImage, testando o Flatpak cedo.

- [ ] Concluir o spike do player no Windows 11 24H2
- [ ] Concluir o spike do player em KDE 6 ou Sway
- [ ] Definir a licença do projeto e a compilação do libmpv
- [ ] Buscar o nome Solune no INPI, EUIPO e USPTO

## Identidade visual

A marca é uma lua crescente com o sol em órbita inclinada: um anel elíptico passa por trás e pela frente da lua, e o sol anda sobre ele. A versão completa vale a partir de 32 px; abaixo disso usa-se o símbolo reduzido, sem órbita. O wordmark é "solune" em Outfit, com o "l" em âmbar. O wordmark ainda precisa ser convertido em curvas antes de uso externo.

| Token | Claro | Escuro (grafite) | Uso |
| --- | --- | --- | --- |
| surface-0 | #f5f3ee | #16171a | Fundo da janela |
| surface-1 | #fbfaf7 | #1d1e22 | Painéis e cartões |
| surface-2 | #ece8df | #27292e | Hover e selecionado |
| ink | #123c46 | #ecedef | Texto principal |
| sun | #f7a23b | #f7a23b | Destaque: no máximo um botão cheio por tela |

Tipografia: Outfit nos títulos, Figtree no texto e JetBrains Mono nos números, todas com licença OFL. Ícones: Lucide, traço de 1,5 px. Cantos de 6, 10, 16 px e pílula. Todos os pares de texto foram medidos com contraste de 4,5:1 ou mais nos dois temas; bordas de controle têm 3:1 ou mais.

Design system completo, com tokens, logo e quatro componentes React: [Solune](https://claude.ai/artifact/6JxrNV7jBX57aXoqcUqtd2). Antes de adotar o nome e o logo, faça a busca de marca nos órgãos oficiais.

## UI e UX

A UI é um controle remoto do daemon: o app funciona com a janela fechada, o wallpaper é o protagonista e só há uma ação com o sol por tela. Tudo que pausa ou poupa energia fica visível e se explica. O canvas com as sete telas interativas, feitas com os componentes reais do design system, está em [Solune UI e UX](https://claude.ai/artifact/T65bCcwBpccbTF9PXA5DgT).

| Tela | Para que serve |
| --- | --- |
| Biblioteca | Ver, filtrar e importar; faixa "Tocando agora" |
| Detalhe e aplicar | Prévia, escolha de monitor, par claro e escuro, custo estimado |
| Monitores e carrossel | Um wallpaper por tela, playlist e intervalo de troca |
| Claro e escuro | Regra de troca (sistema, nascer e pôr do sol, manual) e coleções |
| Regras e desempenho | Pausas, números ao vivo e perfil dos vídeos importados |
| Primeiros passos | Zona de arrastar e checagem do sistema; avisa quando o GNOME precisa de extensão |
| Menu da bandeja | Pausar, próximo, tema e atalhos sem abrir a janela |

Fluxo principal: importar um vídeo (convertido para o perfil leve), abrir o detalhe, escolher o monitor, definir como papel de parede, e, se quiser, ligar um par claro e escuro. Os números e os títulos de wallpaper no canvas são exemplos, não medições.

## Decisões, pontos em aberto e próximos passos

Fechado: nome Solune; logo da lua crescente com o sol em órbita inclinada; tema escuro em grafite; stack Tauri v2 com React; app de código aberto; player em daemon Rust com libmpv; UI/UX aprovada.

Em aberto: comportamento do WorkerW no Windows 11 24H2; suporte ao GNOME por extensão; aceleração por hardware com NVIDIA em Wayland; licença do projeto e do libmpv; busca de marca (INPI, EUIPO, USPTO); wordmark em curvas; lua dos SVGs escuros ainda em tom azulado; telas de configurações gerais e de importação com progresso de conversão.

Próximo passo recomendado: o spike do player, que testa a arquitetura no risco maior antes de qualquer tela virar código. Depois, o scaffold do workspace (daemon, protocolo de comunicação e app Tauri).
