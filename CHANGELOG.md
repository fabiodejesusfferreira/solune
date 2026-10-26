# Changelog

Segue o [SemVer](https://semver.org/lang/pt-BR/). Enquanto a versão começa com 0, a interface e o protocolo do daemon podem mudar entre versões.

## [0.1.0] - 2026-10-25

Primeira versão publicada. Só Linux.

### Funciona
- Biblioteca de vídeos e imagens: importar pelo diálogo ou arrastando, miniaturas, excluir da biblioteca, caminho do arquivo nos detalhes.
- Papel de parede animado pelo libmpv: X11 (testado no Linux Mint com Cinnamon) e layer-shell no Wayland (sem teste em máquina real).
- Pausa automática com app em tela cheia, na bateria e com a tela bloqueada, e liberação do decodificador depois de 5 minutos pausado.
- Carrossel, pares de wallpaper para tema claro e escuro, e início com o sistema.
- Opção de mostrar ou esconder os ícones da área de trabalho (só no Cinnamon).
- Tema claro e escuro do app.

### Limitações
- O Windows ainda não toca o papel de parede.
- As miniaturas e a prévia dos vídeos pedem o `ffmpeg` instalado.
- Só o pacote `.deb`. O AppImage não é publicado por levar bibliotecas GPL.
