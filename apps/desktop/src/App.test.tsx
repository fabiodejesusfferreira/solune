import { createI18n } from "@solune/i18n";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { App } from "./App";
import type { Policy, Status, Usage } from "./daemon/protocol";
import { useDaemon } from "./daemon/store";
import { useLibrary } from "./library/store";

const stopped: Status = { state: "stopped", path: null, pause_reason: null, hwdec: null };
const policy: Policy = {
  settings: {
    pause_on_fullscreen: true,
    pause_on_battery: false,
    pause_on_lock: true,
    release_when_paused: true,
  },
  conditions: { fullscreen: false, on_battery: null, screen_locked: false },
};
const usage: Usage = {
  cpu_permille: 49,
  gpu_permille: 106,
  memory_bytes: 165216256,
  gpu_memory_bytes: 48234496,
  fps: 30,
  released: false,
};

type Handler = (cmd: string, args: Record<string, unknown>) => unknown;

/** Backend de mentira: responde os comandos do app e registra as chamadas. */
function backend(overrides: Record<string, Handler> = {}) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockWindows("main");
  mockIPC(
    (cmd, payload) => {
      const args = (payload ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args });
      const handler = overrides[cmd];
      if (handler) {
        return handler(cmd, args);
      }
      if (cmd === "daemon_status") {
        return stopped;
      }
      if (cmd === "daemon_policy") {
        return policy;
      }
      if (cmd === "daemon_usage") {
        return usage;
      }
      if (cmd === "plugin:autostart|is_enabled") {
        return false;
      }
      return null;
    },
    { shouldMockEvents: true },
  );
  return calls;
}

async function renderApp() {
  const i18n = await createI18n();
  render(
    <I18nextProvider i18n={i18n}>
      <App />
    </I18nextProvider>,
  );
}

beforeEach(() => {
  localStorage.clear();
  useLibrary.setState({ items: [], selected: null });
  useDaemon.setState({ connection: "connecting", status: null, policy: null, error: null });
  Object.defineProperty(navigator, "hardwareConcurrency", { value: 4, configurable: true });
  window.matchMedia ??= () =>
    ({ matches: false, addEventListener() {}, removeEventListener() {} }) as never;
});

afterEach(async () => {
  cleanup();
  // Os listeners saem depois de uma promise; os mocks só podem sair depois.
  await new Promise((resolve) => setTimeout(resolve));
  clearMocks();
});

describe("biblioteca", () => {
  it("começa vazia e mostra o player parado", async () => {
    backend();
    await renderApp();
    expect(screen.getByText("Arraste um vídeo para começar")).toBeInTheDocument();
    expect(await screen.findByText("Parado")).toBeInTheDocument();
  });

  it("importa um vídeo, aplica e acompanha o status por evento", async () => {
    const calls = backend({
      "plugin:dialog|open": () => ["/home/ana/Vídeos/praia.mp4", "/home/ana/notas.txt"],
    });
    const user = userEvent.setup();
    await renderApp();
    await screen.findByText("Parado");

    await user.click(screen.getByRole("button", { name: "Importar vídeo" }));
    expect(await screen.findByText("praia.mp4")).toBeInTheDocument();
    expect(screen.queryByText("notas.txt")).not.toBeInTheDocument();
    expect(screen.getByText("1 wallpaper")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Definir como papel de parede" }));
    expect(calls).toContainEqual({
      cmd: "daemon_play",
      args: { path: "/home/ana/Vídeos/praia.mp4" },
    });

    await act(() =>
      emit("daemon://event", {
        kind: "status_changed",
        status: { ...stopped, state: "playing", path: "/home/ana/Vídeos/praia.mp4" },
      }),
    );
    expect(screen.getByText("Tocando")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /praia\.mp4/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Pausar" })).toBeEnabled();
  });

  it("mostra o erro do daemon com o texto do catálogo", async () => {
    useLibrary.setState({
      items: [{ path: "/sumiu.mp4", name: "sumiu.mp4" }],
      selected: "/sumiu.mp4",
    });
    backend({
      daemon_play: () => {
        throw { code: "file_not_found" };
      },
    });
    const user = userEvent.setup();
    await renderApp();
    await screen.findByText("Parado");

    await user.click(screen.getByRole("button", { name: "Definir como papel de parede" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Não encontrei o arquivo. Ele pode ter sido movido ou apagado.",
    );
  });

  it("avisa quando o daemon não está rodando", async () => {
    backend({
      daemon_status: () => {
        throw { code: "offline" };
      },
    });
    await renderApp();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "O serviço do Solune não está rodando.",
    );
  });
});

describe("regras e desempenho", () => {
  async function openRules() {
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByRole("button", { name: "Regras e desempenho" }));
    expect(screen.getByRole("heading", { name: "Regras e desempenho" })).toBeInTheDocument();
    return user;
  }

  it("mostra o uso do daemon, com a CPU sobre o processador inteiro", async () => {
    backend();
    await openRules();
    // 49 milésimos de um núcleo em 4 núcleos lógicos.
    expect(await screen.findByText("1,2%")).toBeInTheDocument();
    expect(screen.getByText("10,6%")).toBeInTheDocument();
    expect(screen.getByText("158 MB")).toBeInTheDocument();
    expect(screen.getByText("46 MB")).toBeInTheDocument();
    expect(screen.getByText("30 fps")).toBeInTheDocument();
    expect(screen.getByText("pico de 1,2%")).toBeInTheDocument();
    expect(screen.getByRole("navigation")).toContainElement(
      screen.getByRole("button", { name: "Regras e desempenho", current: "page" }),
    );
  });

  it("mostra traço quando o sistema não informa um número", async () => {
    backend({ daemon_usage: () => ({ ...usage, gpu_permille: null, gpu_memory_bytes: null }) });
    await openRules();
    expect(await screen.findAllByText("—")).toHaveLength(2);
    expect(
      screen.getByText("— quer dizer que este sistema não informa o número."),
    ).toBeInTheDocument();
  });

  it("muda uma regra na hora e avisa o que esta sessão não detecta", async () => {
    const calls = backend();
    const user = await openRules();
    const battery = await screen.findByRole("switch", { name: "Pausar na bateria" });
    await screen.findByText("Não encontrei bateria neste computador.");
    expect(battery).toHaveAccessibleDescription(
      "Mostra a última imagem do vídeo até você ligar na tomada. Não encontrei bateria neste computador.",
    );

    await user.click(battery);
    expect(battery).toHaveAttribute("aria-checked", "true");
    expect(calls).toContainEqual({
      cmd: "daemon_set_settings",
      args: { settings: { ...policy.settings, pause_on_battery: true } },
    });
  });

  it("volta a regra como estava se o daemon recusar", async () => {
    backend({
      daemon_set_settings: () => {
        throw { code: "internal" };
      },
    });
    const user = await openRules();
    const lock = await screen.findByRole("switch", { name: "Pausar com a tela bloqueada" });
    await user.click(lock);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Algo deu errado no serviço do Solune. Tente de novo.",
    );
    expect(lock).toHaveAttribute("aria-checked", "true");
  });

  it("segue a política que o daemon anuncia", async () => {
    backend();
    await openRules();
    const fullscreen = await screen.findByRole("switch", {
      name: "Pausar quando um app estiver em tela cheia",
    });
    expect(fullscreen).toHaveAttribute("aria-checked", "true");
    await act(() =>
      emit("daemon://event", {
        kind: "policy_changed",
        policy: { ...policy, settings: { ...policy.settings, pause_on_fullscreen: false } },
      }),
    );
    expect(fullscreen).toHaveAttribute("aria-checked", "false");
  });

  it("liga o início com o sistema pelo plugin", async () => {
    let enabled = false;
    const calls = backend({
      "plugin:autostart|is_enabled": () => enabled,
      "plugin:autostart|enable": () => {
        enabled = true;
        return null;
      },
    });
    const user = await openRules();
    const autostart = screen.getByRole("switch", { name: "Iniciar com o sistema" });
    await vi.waitFor(() => expect(autostart).toBeEnabled());
    await user.click(autostart);
    await vi.waitFor(() => expect(autostart).toHaveAttribute("aria-checked", "true"));
    expect(calls.map((call) => call.cmd)).toContain("plugin:autostart|enable");
  });

  it("abre pela bandeja", async () => {
    backend();
    await renderApp();
    await act(() => emit("app://navigate", "rules"));
    expect(screen.getByRole("heading", { name: "Regras e desempenho" })).toBeInTheDocument();
  });
});
