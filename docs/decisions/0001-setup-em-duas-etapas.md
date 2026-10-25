# 0001. Setup em duas etapas, em volta do spike do player

Data: 25 out. 2026. Status: aceita.

## Contexto

O maior risco do Solune é o vídeo atrás dos ícones: o WorkerW no Windows 11 24H2 e o layer-shell no KDE 6 e no wlroots. A forma como o vídeo chega à tela define o desenho do daemon.

## Decisão

1. **Fase 0, setup mínimo**: workspace Cargo só com o spike, Rust fixado, CI no Linux e no Windows (fmt, clippy, build, cargo-deny) e a documentação no repositório.
2. **Fase 1, spike do player**: mp4 via libmpv atrás dos ícones no Windows 11 24H2 e em layer-shell no KDE ou Sway, com CPU e GPU medidos.
3. **Fase 2, scaffold completo**: monorepo com workspace Cargo e workspace pnpm.

Layout previsto para a fase 2:

```
crates/solune-ipc      protocolo entre app e daemon
crates/solune-core     lógica pura: biblioteca, agendador, regras de pausa
crates/solune-daemon   libmpv, superfícies por plataforma, servidor IPC
apps/desktop           app Tauri v2 com React
packages/tokens        tokens do design system como código
packages/ui            componentes do design system
packages/i18n          catálogos pt-BR
```

Tokens, componentes e i18n não dependem do spike e podem andar em paralelo.

## Consequências

O desenho do daemon só é fechado depois do spike. O código do spike que funcionar é movido para `crates/solune-daemon/src/surface/` na fase 2.
