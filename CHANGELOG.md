# Changelog

Segue o [SemVer](https://semver.org/lang/pt-BR/). Enquanto a versão começa com 0, a interface e o protocolo do daemon podem mudar entre versões.

## [0.1.1] - 2026-10-29

### Mudou
- Executáveis mais enxutos (LTO, `strip` e `panic = "abort"` no perfil release): o daemon passa de 9,7 para 4,6 MB e o app de 19,4 para 8,8 MB. CPU, GPU e RAM do vídeo tocando ficam iguais.
- A grade da biblioteca não é mais redesenhada a cada leitura de uso, e a miniatura não varre mais uma lista a cada render.
- A RAM em Regras e desempenho agora soma todos os processos do Solune: o player (daemon), o app e os processos da webview. Antes era só a RSS do daemon. Um detalhe por processo mostra o que entra no total.
- A memória é medida pela PSS, que reparte entre os processos a memória que eles dividem. Se o sistema não deixar ler a PSS de algum processo, todos entram pela RSS, e a tela avisa que o total fica acima do real. Só no Linux; no Windows a RAM aparece como —.
- Memória em MiB, a mesma unidade do monitor do sistema.

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
