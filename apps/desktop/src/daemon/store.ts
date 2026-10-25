import { create } from "zustand";
import { daemon } from "./api";
import { type CommandErrorCode, commandErrorCode, type DaemonEvent, type Status } from "./protocol";

export type Connection = "connecting" | "online" | "offline" | "incompatible";

/** Erro de uma ação, para mostrar com `error.<código>`. */
export type ActionError = Exclude<CommandErrorCode, "offline" | "incompatible">;

interface DaemonState {
  connection: Connection;
  status: Status | null;
  error: ActionError | null;
  /** Pede o status ao daemon (e inicia o daemon, se ele não está rodando). */
  refresh: () => Promise<void>;
  /** Roda um comando e guarda o erro, se houver. O status novo chega por evento. */
  run: (command: () => Promise<void>) => Promise<void>;
  apply: (event: DaemonEvent) => void;
}

export const useDaemon = create<DaemonState>()((set) => {
  function fail(error: unknown) {
    const code = commandErrorCode(error);
    if (code === "offline" || code === "incompatible") {
      set({ connection: code });
    } else {
      set({ error: code });
    }
  }

  return {
    connection: "connecting",
    status: null,
    error: null,
    refresh: async () => {
      try {
        const status = await daemon.status();
        set({ connection: "online", status });
      } catch (error) {
        fail(error);
      }
    },
    run: async (command) => {
      set({ error: null });
      try {
        await command();
      } catch (error) {
        fail(error);
      }
    },
    apply: (event) => {
      if (event.kind === "status_changed") {
        set({ connection: "online", status: event.status });
      } else {
        set({ error: event.error === "incompatible" ? "internal" : event.error });
      }
    },
  };
});
