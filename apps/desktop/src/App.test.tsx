import { createI18n } from "@solune/i18n";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { App } from "./App";
import type { Policy, Status, Usage } from "./daemon/protocol";
import { useDaemon } from "./daemon/store";
import { useLibrary } from "./library/store";

const stopped: Status = {
  state: "stopped",
  path: null,
  pause_reason: null,
  hwdec: null,
  theme: "light",
};
const policy: Policy = {
  settings: {
    pause_on_fullscreen: true,
    pause_on_battery: false,
    pause_on_lock: true,
    release_when_paused: true,
    sound: false,
    show_desktop_icons: true,
    schedule: { mode: "system" },
  },
  conditions: { fullscreen: false, on_battery: null, screen_locked: false, dark: false },
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
  mockConvertFileSrc("linux");
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
      if (cmd === "media_list") {
        return (args.paths as string[]).filter((path) => /\.(mp4|png)$/.test(path));
      }
      if (cmd === "media_allow") {
        return (args.paths as string[]).map(() => 1024);
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
  useLibrary.setState({
    items: [],
    allowed: [],
    thumbs: {},
    carousel: { paths: [], every_minutes: 30, shuffle: false, on: false },
  });
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

const praia = { path: "/home/ana/Vídeos/praia.mp4", name: "praia.mp4", kind: "video" as const };
const serra = { path: "/home/ana/Imagens/serra.png", name: "serra.png", kind: "image" as const };
const playList = (items: { path: string; dark: string | null }[], every_minutes = 0) => ({
  cmd: "daemon_play_list",
  args: { list: { items, every_minutes, shuffle: false } },
});

describe("biblioteca", () => {
  it("começa vazia, nos primeiros passos", async () => {
    backend();
    await renderApp();
    expect(
      screen.getByRole("heading", { name: "Arraste um vídeo ou imagem para começar" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });

  it("importa, abre o detalhe, aplica e acompanha o status por evento", async () => {
    const calls = backend({
      "plugin:dialog|open": () => [praia.path, "/home/ana/notas.txt"],
    });
    const user = userEvent.setup();
    await renderApp();

    await user.click(screen.getByRole("button", { name: "Escolher arquivo" }));
    expect(await screen.findByRole("button", { name: /praia\.mp4/ })).toBeInTheDocument();
    expect(screen.queryByText("notas.txt")).not.toBeInTheDocument();
    expect(screen.getByText("1 de 1 item")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /praia\.mp4/ }));
    expect(screen.getByRole("heading", { name: "praia.mp4" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Definir como papel de parede" }));
    expect(calls).toContainEqual(playList([{ path: praia.path, dark: null }]));

    await act(() =>
      emit("daemon://event", {
        kind: "status_changed",
        status: { ...stopped, state: "playing", path: praia.path },
      }),
    );
    expect(screen.getByText("Aplicado em todos os monitores")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Voltar à biblioteca" }));
    expect(screen.getByRole("button", { name: /praia\.mp4/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Pausar" })).toBeEnabled();
  });

  it("exclui da biblioteca pelo cartão", async () => {
    useLibrary.setState({ items: [praia, serra] });
    backend();
    const user = userEvent.setup();
    await renderApp();
    await user.click(
      screen.getAllByRole("button", { name: "Excluir da biblioteca" })[0] as HTMLElement,
    );
    expect(screen.queryByRole("button", { name: /praia\.mp4/ })).not.toBeInTheDocument();
    expect(screen.getByText("1 de 1 item")).toBeInTheDocument();
  });

  it("mostra o caminho do arquivo e troca o tema do app", async () => {
    useLibrary.setState({ items: [praia] });
    backend();
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByRole("switch", { name: "Modo escuro do app" }));
    expect(document.documentElement.dataset.theme).toBeDefined();
    await user.click(screen.getByRole("button", { name: /praia\.mp4/ }));
    expect(screen.getByText(praia.path)).toBeInTheDocument();
  });

  it("filtra por busca e por tipo", async () => {
    useLibrary.setState({ items: [praia, serra] });
    backend();
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByRole("button", { name: "Imagens" }));
    expect(screen.getByText("1 de 2 itens")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /praia\.mp4/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Tudo" }));
    await user.type(screen.getByRole("searchbox", { name: "Buscar na biblioteca" }), "pra");
    expect(screen.getByRole("button", { name: /praia\.mp4/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /serra\.png/ })).not.toBeInTheDocument();
  });

  it("aplica o par claro e escuro", async () => {
    useLibrary.setState({ items: [praia, serra] });
    const calls = backend();
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByRole("button", { name: /praia\.mp4/ }));
    await user.click(screen.getByRole("button", { name: "Escuro" }));
    await user.selectOptions(screen.getByLabelText("Par para o tema claro"), serra.path);
    await user.click(screen.getByRole("button", { name: "Definir como papel de parede" }));
    expect(calls).toContainEqual(playList([{ path: serra.path, dark: praia.path }]));
    expect(useLibrary.getState().items.find((i) => i.path === serra.path)?.theme).toBe("light");
  });

  it("mostra o erro do daemon com o texto do catálogo", async () => {
    useLibrary.setState({ items: [{ path: "/sumiu.mp4", name: "sumiu.mp4", kind: "video" }] });
    backend({
      daemon_play_list: () => {
        throw { code: "file_not_found" };
      },
    });
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByRole("button", { name: /sumiu\.mp4/ }));
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

describe("monitores e temas", () => {
  it("monta e toca o carrossel", async () => {
    useLibrary.setState({ items: [praia, serra] });
    const calls = backend();
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByRole("button", { name: "Monitores" }));
    await user.click(screen.getByRole("button", { name: "Carrossel" }));
    const add = screen.getByLabelText("Adicionar da biblioteca");
    await user.selectOptions(add, praia.path);
    await user.selectOptions(screen.getByLabelText("Adicionar da biblioteca"), serra.path);
    await user.click(screen.getByRole("button", { name: "Tocar o carrossel" }));
    expect(calls).toContainEqual(
      playList(
        [
          { path: praia.path, dark: null },
          { path: serra.path, dark: null },
        ],
        30,
      ),
    );
    expect(useLibrary.getState().carousel.on).toBe(true);
  });

  it("troca a agenda de temas", async () => {
    useLibrary.setState({ items: [praia] });
    const calls = backend();
    const user = userEvent.setup();
    await renderApp();
    await user.click(screen.getByRole("button", { name: "Claro e escuro" }));
    const times = screen.getByRole("button", { name: "Por horário" });
    await vi.waitFor(() => expect(times).toBeEnabled());
    await user.click(times);
    expect(calls).toContainEqual({
      cmd: "daemon_set_settings",
      args: {
        settings: { ...policy.settings, schedule: { mode: "times", light_at: 420, dark_at: 1140 } },
      },
    });
  });
});

describe("regras e desempenho", () => {
  async function openRules() {
    useLibrary.setState({ items: [praia] });
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
