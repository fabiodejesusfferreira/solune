# 0004. Política de pausa e tela Regras e desempenho

Data: 25 out. 2026. Status: aceita.

## Contexto

O passo 3 do plano (`docs/Solune.md`) é a política de desempenho: pausar quando ninguém vê o wallpaper, liberar o decodificador depois de 5 minutos pausado e mostrar ao usuário quanto o Solune usa. O Fábio pediu a fase inteira em 25 out. 2026. As escolhas abaixo foram feitas na implementação.

## Decisão

- **Quem decide.** O daemon. A lógica fica em `solune-core` (`Pauses`): pausar pelo app vence tudo; depois vêm tela bloqueada, tela cheia e bateria. Retomar pelo app desfaz as pausas automáticas ativas naquele momento, até o sinal sumir e voltar. Um wallpaper novo zera as duas coisas.
- **Sinais no Linux.** Cada um tem um observador que só avisa quando o valor muda, e `null` quer dizer que a sessão não deixa detectar (a regra não vale ali).
  - Tela cheia no X11: `_NET_WM_STATE_FULLSCREEN` de uma janela da área de trabalho atual, que não esteja oculta, pelo EWMH, sem polling (PropertyNotify).
  - Tela cheia no Wayland: o protocolo wlr-foreign-toplevel-management (Sway, Hyprland). O KDE Plasma não oferece esse protocolo; lá fica `null`.
  - Bateria: o sysfs (`/sys/class/power_supply`) a cada 10 s, sem UPower. Baterias de periféricos (`scope=Device`) não contam.
  - Tela bloqueada: o `ActiveChanged` do protetor de tela no D-Bus da sessão (Cinnamon, GNOME, MATE e o nome freedesktop, que o KDE usa) e o `LockedHint` do logind. Basta um dizer que está bloqueada.
- **Liberar depois de 5 minutos.** A superfície guarda o quadro atual num framebuffer OpenGL nosso e passa a mostrar só ele. O daemon para o arquivo no mpv (sai o demuxer e o decodificador) e desliga o render do libmpv (saem texturas, framebuffers e o interop com a GPU). Depois devolve ao sistema a memória que o processo soltou (`malloc_trim`). O contexto EGL e a janela continuam; se a tela muda de tamanho, o quadro guardado é desenhado de novo, esticado. Ao retomar, a superfície religa o render, e o vídeo abre já na posição salva (opção `start` do mpv); o quadro guardado fica até o primeiro quadro novo.
  - A primeira versão trocava o vídeo por uma foto do quadro em PNG, carregada no mpv. No notebook do Fábio a RAM do daemon subiu de 149 para 201 MiB com isso e não desceu mais, e a memória de vídeo ficou acima da que ele usava tocando. Por isso a troca.
- **Preferências e último wallpaper.** Ficam com o daemon, em `settings.json` e `state.json` na pasta de configuração do usuário (`~/.config/solune`, `%APPDATA%\Solune` no Windows; `SOLUNE_CONFIG_DIR` troca). Ao iniciar, o daemon volta a tocar o último wallpaper.
- **Uso de recursos.** O daemon mede o próprio processo: CPU e memória por `/proc/self`, GPU e memória de vídeo pelo fdinfo do DRM (`drm-engine-*`, `drm-resident-*`), quadros pelo `estimated-vf-fps` do mpv. A tela mostra a CPU como fatia do processador inteiro (dividida pelos núcleos lógicos) e a GPU como o motor mais ocupado. O protocolo leva a CPU em milésimos de um núcleo, e a CLI mostra em % de um núcleo.
- **Leitura só com a janela à vista.** A tela pede o uso uma vez por segundo, e só enquanto `document.visibilityState` é `visible`. Na bandeja, o app não pergunta nada.
- **Iniciar com o sistema.** O plugin de autostart do Tauri, com o argumento `--hidden`: no login o app abre só na bandeja e o daemon já volta com o último wallpaper.
- **Perfil dos vídeos importados.** A seção do canvas (Leve, Equilibrado, Original) ficou de fora: a conversão na importação ainda não existe.

## Consequências

- Protocolo na versão 2: `get_policy`, `set_settings`, `usage` e o evento `policy_changed`. Um app de uma versão e um daemon da outra recusam a conexão no `hello`.
- Windows: os três sinais ficam `null` até a fase do WorkerW.
- O KDE Plasma não pausa em tela cheia por enquanto. Alternativa futura: o D-Bus do KWin.
- A memória de vídeo liberada e o tempo para retomar ainda precisam de medição em hardware real.
