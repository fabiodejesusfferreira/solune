import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  type DaemonEvent,
  type Playlist,
  type Policy,
  parseEvent,
  parsePolicy,
  parseStatus,
  parseUsage,
  type Settings,
  type Status,
  type Usage,
} from "./protocol";

/** Comandos do app que viram pedidos ao daemon (`src-tauri/src/daemon.rs`). */
export const daemon = {
  status: async (): Promise<Status> => parseStatus(await invoke("daemon_status")),
  policy: async (): Promise<Policy> => parsePolicy(await invoke("daemon_policy")),
  usage: async (): Promise<Usage> => parseUsage(await invoke("daemon_usage")),
  setSettings: (settings: Settings) => invoke<void>("daemon_set_settings", { settings }),
  playList: (list: Playlist) => invoke<void>("daemon_play_list", { list }),
  next: () => invoke<void>("daemon_next"),
  pause: () => invoke<void>("daemon_pause"),
  resume: () => invoke<void>("daemon_resume"),
  stop: () => invoke<void>("daemon_stop"),
};

/** Memória de cada parte do Solune, em bytes; `null` onde a parte não existe agora. */
export interface AppMemory {
  /** `pss` divide a memória compartilhada entre os processos; `rss` conta inteira em cada um. */
  metric: "pss" | "rss";
  daemon: number | null;
  app: number | null;
  webview: number | null;
  webview_processes: number;
  other: number | null;
  other_processes: number;
}

/**
 * Mede o app, os filhos dele (a webview) e o daemon pelo pid; `null` fora do
 * Linux (`src-tauri/src/memory.rs`).
 */
export const appMemory = (daemonPid: number | null) =>
  invoke<AppMemory | null>("app_memory", { daemonPid });

/** Libera os arquivos para miniaturas e prévia, e devolve o tamanho de cada um (`src-tauri/src/media.rs`). */
export const mediaAllow = (paths: string[]) => invoke<(number | null)[]>("media_allow", { paths });

export interface VideoThumb {
  /** Imagem JPEG, já liberada para o protocolo `asset:`. */
  path: string;
  width: number;
  height: number;
  duration: number;
}

/** Miniatura e dados de um vídeo; `null` se o `ffmpeg` não conseguir (`src-tauri/src/media.rs`). */
export const mediaThumb = (path: string) => invoke<VideoThumb | null>("media_thumb", { path });

/** Os arquivos de mídia entre os caminhos, abrindo as pastas. */
export const mediaList = (paths: string[]) => invoke<string[]>("media_list", { paths });

export type Support = "ready" | "needs_extension" | "unsupported";
export const desktopSession = () => invoke<{ name: string; support: Support }>("desktop_session");

export function onDaemonEvent(handler: (event: DaemonEvent) => void): Promise<UnlistenFn> {
  return listen("daemon://event", (event) => handler(parseEvent(event.payload)));
}

/** A conexão caiu. O próximo comando reconecta, iniciando o daemon se preciso. */
export function onDaemonDisconnected(handler: () => void): Promise<UnlistenFn> {
  return listen("daemon://disconnected", () => handler());
}
