import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  type DaemonEvent,
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
  play: (path: string) => invoke<void>("daemon_play", { path }),
  pause: () => invoke<void>("daemon_pause"),
  resume: () => invoke<void>("daemon_resume"),
  stop: () => invoke<void>("daemon_stop"),
};

export function onDaemonEvent(handler: (event: DaemonEvent) => void): Promise<UnlistenFn> {
  return listen("daemon://event", (event) => handler(parseEvent(event.payload)));
}

/** A conexão caiu. O próximo comando reconecta, iniciando o daemon se preciso. */
export function onDaemonDisconnected(handler: () => void): Promise<UnlistenFn> {
  return listen("daemon://disconnected", () => handler());
}
