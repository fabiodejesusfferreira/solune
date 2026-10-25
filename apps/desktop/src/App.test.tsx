import { createI18n } from "@solune/i18n";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { I18nextProvider } from "react-i18next";
import { App } from "./App";
import type { Status } from "./daemon/protocol";
import { useDaemon } from "./daemon/store";
import { useLibrary } from "./library/store";

const stopped: Status = { state: "stopped", path: null, pause_reason: null, hwdec: null };

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
  useDaemon.setState({ connection: "connecting", status: null, error: null });
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
