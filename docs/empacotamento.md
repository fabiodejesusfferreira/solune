# Empacotamento

Como gerar os instaladores do Solune em cada formato, o que cada sistema consegue gerar e onde o daemon e o libmpv entram em cada pacote.

Os comandos do Tauri foram conferidos na documentação oficial do Tauri 2 em 25 out. 2026 (links no fim). O que diz "gerado" saiu de um Ubuntu 24.04 na nuvem em 25 out. 2026, e eu conferi o conteúdo do pacote, mas não instalei nem abri. Nenhum pacote foi testado num computador de verdade ainda.

## Resumo

| Formato | Gera em | Comando | Estado |
| --- | --- | --- | --- |
| `.deb` | Linux | `pnpm build` | Gerado, 7,7 MiB. Falta instalar e abrir num computador |
| `.rpm` | Linux | `pnpm build --bundles rpm` | Gerado, 7,7 MiB. Falta instalar e abrir num Fedora ou openSUSE |
| AppImage | Linux | `pnpm build --bundles appimage` | Gerado, 147 MiB, mas não pode ser distribuído assim: leva bibliotecas GPL (veja abaixo) |
| `.exe` (NSIS) | Windows (e Linux, com ressalvas) | `pnpm build` | Não gerado: falta o libmpv para Windows |
| `.msi` (WiX) | Só Windows | `pnpm build --bundles msi` | Não gerado: falta o libmpv para Windows |
| Flatpak | Linux | `flatpak-builder` com um manifesto | Não configurado |

Formatos de Linux só saem de um Linux, e o `.msi` só sai de um Windows, porque o WiX só roda no Windows. O NSIS pode ser gerado a partir do Linux, com ressalvas (veja abaixo).

## Como o build funciona

O `pnpm build` faz três coisas:

1. Gera os tokens do design system (`pnpm tokens`).
2. Compila o daemon em release e copia para `apps/desktop/src-tauri/binaries/solune-daemon-<alvo>`, onde `<alvo>` é o alvo do Rust, como `x86_64-unknown-linux-gnu` ([`scripts/stage-daemon.mjs`](../scripts/stage-daemon.mjs)).
3. Roda `tauri build --config src-tauri/tauri.bundle.json`, que compila a interface e o app e gera os pacotes.

O [`tauri.bundle.json`](../apps/desktop/src-tauri/tauri.bundle.json) entra por cima do [`tauri.conf.json`](../apps/desktop/src-tauri/tauri.conf.json) só no build de instalador. Ele declara o daemon como `externalBin` (o Tauri chama de sidecar) e põe o `libmpv2` como dependência do `.deb`. Assim, o `cargo build` comum não exige o daemon compilado.

Os formatos que saem sem argumento vêm de `bundle.targets` no `tauri.conf.json`: hoje `["deb", "nsis"]`. Cada sistema gera só os da sua plataforma. Para pedir outros, passe `--bundles` com a lista separada por vírgula. O pnpm repassa o argumento ao `tauri build`:

```sh
pnpm build --bundles deb,rpm,appimage
```

Os pacotes saem em `target/release/bundle/<formato>/`, na raiz do repositório.

## Linux: deb, rpm e AppImage

### Preparar

Os mesmos pacotes do [README](../README.md#preparar-o-ambiente): Rust, Node.js 24, pnpm 12, o `libmpv2` e as bibliotecas da webview e da bandeja. No Debian, Ubuntu e Mint:

```sh
sudo apt install libmpv2 libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
pnpm bootstrap
```

O `.rpm` e o AppImage saem do mesmo Ubuntu. O `.rpm` não precisou de nada a mais. Para o AppImage, o Tauri baixa o `linuxdeploy` na primeira vez, então precisa de rede. No Ubuntu da nuvem eu também tinha instalado o `file` e o `xdg-utils`, e não conferi se eles são obrigatórios.

### Versão da glibc

O pacote só roda em sistemas com a glibc igual ou mais nova que a do sistema onde ele foi gerado. A documentação do Tauri recomenda gerar no sistema mais antigo que se quer atender e cita o Ubuntu 22.04 e o Debian 12. No Solune, o piso também depende do libmpv: o daemon linka com o `libmpv.so.2`, e o pacote `libmpv2` existe no Ubuntu 24.04 (conferido aqui). Não conferi o nome do pacote no Ubuntu 22.04 e no Debian 12.

### deb

```sh
pnpm build
```

- O app vai para `/usr/bin/solune` e o daemon para `/usr/bin/solune-daemon`, ao lado. O app procura o daemon na mesma pasta do executável.
- O libmpv não entra no pacote. O `.deb` declara `libmpv2` como dependência, e o apt instala junto.
- Conferido num `.deb` gerado em 25 out. 2026: `Depends: libmpv2, libayatana-appindicator3-1, libwebkit2gtk-4.1-0, libgtk-3-0`.

### rpm

```sh
pnpm build --bundles rpm
```

- O daemon fica ao lado do app, como no `.deb`.
- O libmpv não entra no pacote. O [`tauri.bundle.json`](../apps/desktop/src-tauri/tauri.bundle.json) declara a dependência `libmpv.so.2()(64bit)`, pelo nome da biblioteca e não do pacote, porque o pacote muda de nome entre distribuições (no Fedora é o `mpv-libs`, que fornece o `libmpv.so.2`).
- Conferido no `.rpm` gerado: ele pede `libmpv.so.2()(64bit)`, `libwebkit2gtk-4.1.so.0()(64bit)`, `libgtk-3.so.0()(64bit)` e `libayatana-appindicator3.so.1()(64bit)`.
- Para assinar, o Tauri lê a chave GPG em `TAURI_SIGNING_RPM_KEY` (e a senha em `TAURI_SIGNING_RPM_KEY_PASSPHRASE`).
- Para conferir o pacote: `rpm -qip` (informações), `rpm -qlp` (arquivos) e `rpm -qp --requires` (dependências).

### AppImage

```sh
pnpm build --bundles appimage
```

O AppImage leva as dependências dentro do arquivo em vez de usar as do sistema, então fica bem maior que o `.deb`. A pessoa marca o arquivo como executável e abre, sem instalar.

O que o AppImage gerado em 25 out. 2026 leva:

- O app e o daemon em `usr/bin`, e o `libmpv.so.2` em `usr/lib`. O daemon acha o libmpv de dentro do AppImage pelo `RUNPATH` (`$ORIGIN/../lib`), que o `linuxdeploy` grava.
- Tudo de que o libmpv depende no sistema onde foi gerado, cerca de 300 bibliotecas, com os avisos de copyright em `usr/share/doc`.

Por isso ele não pode ser distribuído como está. O libmpv e o FFmpeg do Ubuntu são builds GPL: entre as bibliotecas copiadas estão o `libx264`, o `libx265` e o `librubberband`, que são GPL. O [ADR 0002](decisions/0002-licenca-mit-e-libmpv.md) deixa builds GPL de fora. Para um AppImage publicável, falta gerar com um libmpv e um FFmpeg compilados como LGPL, e permitir trocar a `.so`, como a LGPL pede.

O AppImage também leva o `libva` do sistema onde foi gerado. Não testei se a aceleração por hardware funciona com os drivers de outro computador.

O AppImage para ARM só sai de uma máquina ARM: o `linuxdeploy` não faz compilação cruzada.

## Windows: exe (NSIS) e msi (WiX)

O Windows ainda não toca o papel de parede (o WorkerW não foi implementado), e o CI só roda o clippy no Windows, sem linkar. Então nada disto foi testado: é o caminho previsto.

### Preparar

- Microsoft C++ Build Tools, com a opção "Desktop development with C++".
- WebView2: já vem no Windows 10 (a partir da 1803) e no 11.
- Rust, Node.js 24 e pnpm 12.
- Um build LGPL do libmpv, com a biblioteca de importação (`mpv.lib`, que é o nome que o [`build.rs`](../crates/solune-daemon/build.rs) pede) e a DLL. De onde vem esse build ainda não foi decidido ([ADR 0002](decisions/0002-licenca-mit-e-libmpv.md)).
- Para o `.msi`: o recurso opcional VBSCRIPT ligado, em Configurações, Apps, Recursos opcionais, Mais recursos do Windows. Sem ele, o WiX falha com `failed to run light.exe`. O Tauri usa o WiX 3 e baixa sozinho.

### Gerar

No PowerShell, na raiz do repositório:

```powershell
$env:MPV_LIB_DIR = "C:\caminho\do\libmpv"   # pasta com o mpv.lib
pnpm bootstrap
pnpm build                        # instalador NSIS (.exe)
pnpm build --bundles msi          # instalador MSI
pnpm build --bundles nsis,msi     # os dois
```

Os instaladores saem em `target\release\bundle\nsis\` e `target\release\bundle\msi\`.

### Onde entram o daemon e o libmpv

- O daemon (`solune-daemon.exe`) fica na pasta de instalação, ao lado do `solune.exe`.
- A DLL do libmpv não entra sozinha: falta declarar no `tauri.bundle.json`. A ideia é usar `bundle.resources`, porque no Windows a pasta de recursos é a pasta do executável, e o Windows procura a DLL primeiro ali. Ainda não feito nem testado.
- Junto da DLL vai o aviso de licença do libmpv (LGPL), e a pessoa precisa poder trocar a DLL. Instalar como arquivo solto na pasta do app atende a isso.

### NSIS a partir do Linux

O Tauri consegue gerar o instalador NSIS no Linux, com o `cargo-xwin`. A documentação avisa que é menos testado que gerar no Windows e que assinar o instalador pede uma ferramenta externa.

```sh
sudo apt install nsis lld llvm
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin
pnpm --filter @solune/desktop tauri build --config src-tauri/tauri.bundle.json \
  --runner cargo-xwin --target x86_64-pc-windows-msvc
```

No Solune, isso ainda não funciona como está: o `scripts/stage-daemon.mjs` compila o daemon para o sistema atual, não para o Windows, e o daemon precisa do `mpv.lib`. Precisaria compilar o daemon com o mesmo alvo e copiar como `solune-daemon-x86_64-pc-windows-msvc.exe`.

## Flatpak

Não está configurado. O caminho que a documentação do Tauri descreve:

1. Gerar o `.deb`.
2. Escrever um arquivo AppStream MetaInfo (nome, descrição, capturas de tela, licença).
3. Escrever um manifesto do `flatpak-builder` sobre o runtime `org.gnome.Platform`, que extrai o `.deb` para `/app`.
4. Gerar e instalar:

```sh
sudo apt install flatpak flatpak-builder
flatpak install flathub org.gnome.Platform//47 org.gnome.Sdk//47
flatpak-builder --force-clean --user --repo flatpak-repo flatpak manifesto.yml
flatpak run app.solune.desktop
```

O runtime do GNOME não traz o libmpv. O manifesto teria de compilar o mpv como um módulo próprio. A documentação desaconselha copiar uma `.so` do sistema para dentro do Flatpak.

O Solune tem pontos a mais que pedem teste antes de valer a pena, todos não verificados:

- O daemon desenha na camada de fundo do layer-shell (Wayland) ou numa janela de área de trabalho (X11). Precisa de `--socket=wayland`, `--socket=fallback-x11` e `--device=dri`, e o compositor pode esconder o layer-shell de apps em sandbox.
- A pausa automática lê `/sys/class/power_supply` e fala com o D-Bus do protetor de tela e do logind, o que pede permissões extras no manifesto.
- O início com o sistema usa o plugin de autostart do Tauri, que grava em `~/.config/autostart`. No Flatpak, isso passa pelo portal de segundo plano.
- A bandeja pede `--talk-name=org.kde.StatusNotifierWatcher`.

## O que falta

- Instalar e abrir o `.deb` num computador (o Linux Mint do Fábio).
- Decidir de onde vem o libmpv LGPL do Windows, declarar a DLL e o aviso de licença no pacote e gerar o NSIS e o MSI.
- Gerar o AppImage com um libmpv e um FFmpeg LGPL, sem as bibliotecas GPL que vêm do Ubuntu.
- Gerar os pacotes no CI, num sistema mais antigo, para baixar o piso da glibc.
- Incluir o aviso de licença do libmpv nos pacotes de Linux que o levarem dentro.

## Fontes

- [Distribute](https://v2.tauri.app/distribute/), [Debian](https://v2.tauri.app/distribute/debian/), [RPM](https://v2.tauri.app/distribute/rpm/), [AppImage](https://v2.tauri.app/distribute/appimage/), [Windows Installer](https://v2.tauri.app/distribute/windows-installer/) e [Flathub](https://v2.tauri.app/distribute/flatpak/), na documentação do Tauri 2.
- [Command Line Interface](https://v2.tauri.app/reference/cli/) (`tauri build`), [Prerequisites](https://v2.tauri.app/start/prerequisites/), [Embedding External Binaries](https://v2.tauri.app/develop/sidecar/) e [Embedding Additional Files](https://v2.tauri.app/develop/resources/).
- [mpv-libs no Fedora](https://packages.fedoraproject.org/pkgs/mpv/mpv-libs/), que fornece o `libmpv.so.2`.
