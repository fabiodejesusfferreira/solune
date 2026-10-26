import { create } from "zustand";
import { daemon } from "./api";
import {
  type CommandErrorCode,
  commandErrorCode,
  type DaemonEvent,
  type Policy,
  type Settings,
  type Status,
} from "./protocol";

export type Connection = "connecting" | "online" | "offline" | "incompatible";

/** Erro de uma ação, para mostrar com `error.<código>`. */
export type ActionError = Exclude<CommandErrorCode, "offline" | "incompatible">;

interface DaemonState {
  connection: Connection;
  status: Status | null;
  policy: Policy | null;
  error: ActionError | null;
  /** Pede o status e a política ao daemon (e inicia o daemon, se ele não está rodando). */
  refresh: () => Promise<void>;
  /** Roda um comando e guarda o erro, se houver. O status novo chega por evento. */
  run: (command: () => Promise<void>) => Promise<void>;
  /** Muda uma preferência na hora. Se o daemon recusar, volta como estava. */
  setSetting: <K extends keyof Settings>(name: K, value: Settings[K]) => Promise<void>;
  apply: (event: DaemonEvent) => void;
}

export const useDaemon = create<DaemonState>()((set, get) => {
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
    policy: null,
    error: null,
    refresh: async () => {
      try {
        const [status, policy] = await Promise.all([daemon.status(), daemon.policy()]);
        set({ connection: "online", status, policy });
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
    setSetting: async (name, value) => {
      const before = get().policy;
      if (!before) {
        return;
      }
      const settings = { ...before.settings, [name]: value };
      set({ policy: { ...before, settings }, error: null });
      try {
        await daemon.setSettings(settings);
      } catch (error) {
        set({ policy: before });
        fail(error);
      }
    },
    apply: (event) => {
      if (event.kind === "status_changed") {
        set({ connection: "online", status: event.status });
      } else if (event.kind === "policy_changed") {
        set({ policy: event.policy });
      } else {
        set({ error: event.error === "incompatible" ? "internal" : event.error });
      }
    },
  };
});
