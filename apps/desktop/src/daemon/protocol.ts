/**
 * Tipos do protocolo do daemon, escritos à mão a partir de
 * `crates/solune-ipc/src/protocol.rs`. O `protocol.test.ts` confere estes
 * tipos contra os exemplos em JSON que o Rust grava em
 * `crates/solune-ipc/fixtures`: se o Rust mudar, o teste quebra.
 */

export const playerStates = ["stopped", "playing", "paused"] as const;
export type PlayerState = (typeof playerStates)[number];

/** Motivos de pausa. Cada um tem texto em `pause_reason.<código>`. */
export const pauseReasons = ["user", "fullscreen", "battery", "locked"] as const;
export type PauseReason = (typeof pauseReasons)[number];

/** Erros do daemon. Cada um tem texto em `error.<código>`, exceto `incompatible`. */
export const errorCodes = [
  "file_not_found",
  "unsupported_format",
  "surface_unavailable",
  "incompatible",
  "internal",
] as const;
export type ErrorCode = (typeof errorCodes)[number];

export const themes = ["light", "dark"] as const;
export type Theme = (typeof themes)[number];

export interface Status {
  state: PlayerState;
  path: string | null;
  pause_reason: PauseReason | null;
  /** Decodificação por hardware em uso, como o mpv informa. */
  hwdec: string | null;
  /** O tema que vale agora, pela agenda. */
  theme: Theme;
}

/** Quando usar o arquivo do tema claro ou do escuro. Horários em minutos desde a meia-noite. */
export type Schedule =
  | { mode: "system" }
  | { mode: "fixed"; theme: Theme }
  | { mode: "times"; light_at: number; dark_at: number };

/** O que tocar: um item, ou um carrossel que troca a cada `every_minutes` (0: a cada login). */
export interface Playlist {
  items: { path: string; dark: string | null }[];
  every_minutes: number;
  shuffle: boolean;
}

/** Preferências que ligam e desligam. As de pausa têm texto em `rules.<nome>`. */
export const settingNames = [
  "pause_on_fullscreen",
  "pause_on_battery",
  "pause_on_lock",
  "release_when_paused",
  "sound",
  "show_desktop_icons",
] as const;
export type SettingName = (typeof settingNames)[number];
export type Settings = Record<SettingName, boolean> & { schedule: Schedule };

/** O que o daemon detectou do sistema. `null` quando não dá para detectar nesta sessão. */
export interface Conditions {
  fullscreen: boolean | null;
  on_battery: boolean | null;
  screen_locked: boolean | null;
  /** O sistema está no tema escuro. */
  dark: boolean | null;
}

export interface Policy {
  settings: Settings;
  conditions: Conditions;
}

/** Uso de recursos do daemon. `null` quando o sistema não informa. */
export interface Usage {
  /** Milésimos de um núcleo: 1000 é um núcleo inteiro. */
  cpu_permille: number | null;
  /** Milésimos do motor da GPU mais ocupado. */
  gpu_permille: number | null;
  memory_bytes: number | null;
  gpu_memory_bytes: number | null;
  fps: number | null;
  /** O decodificador foi liberado depois de muito tempo em pausa. */
  released: boolean;
}

export type DaemonEvent =
  | { kind: "status_changed"; status: Status }
  | { kind: "playback_failed"; error: ErrorCode }
  | { kind: "policy_changed"; policy: Policy };

/** Erro dos comandos do app: os do daemon, mais `offline` quando não há conexão. */
export type CommandErrorCode = ErrorCode | "offline";

export class ProtocolError extends Error {
  override name = "ProtocolError";
}

type Json = Record<string, unknown>;

function isObject(value: unknown): value is Json {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function oneOf<T extends string>(list: readonly T[], value: unknown): value is T {
  return typeof value === "string" && (list as readonly string[]).includes(value);
}

function nullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function nullableBoolean(value: unknown): value is boolean | null {
  return value === null || typeof value === "boolean";
}

function isMinute(value: unknown): value is number {
  return Number.isInteger(value) && (value as number) >= 0 && (value as number) < 24 * 60;
}

function parseSchedule(value: unknown): Schedule | null {
  if (!isObject(value)) {
    return null;
  }
  if (value.mode === "system") {
    return { mode: "system" };
  }
  if (value.mode === "fixed" && oneOf(themes, value.theme)) {
    return { mode: "fixed", theme: value.theme };
  }
  if (value.mode === "times" && isMinute(value.light_at) && isMinute(value.dark_at)) {
    return { mode: "times", light_at: value.light_at, dark_at: value.dark_at };
  }
  return null;
}

function nullableCount(value: unknown): value is number | null {
  return value === null || (Number.isInteger(value) && (value as number) >= 0);
}

export function parseStatus(value: unknown): Status {
  if (
    isObject(value) &&
    oneOf(playerStates, value.state) &&
    nullableString(value.path) &&
    (value.pause_reason === null || oneOf(pauseReasons, value.pause_reason)) &&
    nullableString(value.hwdec) &&
    oneOf(themes, value.theme)
  ) {
    return {
      state: value.state,
      path: value.path,
      pause_reason: value.pause_reason,
      hwdec: value.hwdec,
      theme: value.theme,
    };
  }
  throw new ProtocolError(`status inválido: ${JSON.stringify(value)}`);
}

export function parsePolicy(value: unknown): Policy {
  if (isObject(value) && isObject(value.settings) && isObject(value.conditions)) {
    const { settings, conditions } = value;
    const schedule = parseSchedule(settings.schedule);
    if (
      schedule &&
      settingNames.every((name) => typeof settings[name] === "boolean") &&
      nullableBoolean(conditions.fullscreen) &&
      nullableBoolean(conditions.on_battery) &&
      nullableBoolean(conditions.screen_locked) &&
      nullableBoolean(conditions.dark)
    ) {
      return {
        settings: {
          ...(Object.fromEntries(
            settingNames.map((name) => [name, settings[name] as boolean]),
          ) as Record<SettingName, boolean>),
          schedule,
        },
        conditions: {
          fullscreen: conditions.fullscreen,
          on_battery: conditions.on_battery,
          screen_locked: conditions.screen_locked,
          dark: conditions.dark,
        },
      };
    }
  }
  throw new ProtocolError(`política inválida: ${JSON.stringify(value)}`);
}

export function parseUsage(value: unknown): Usage {
  if (
    isObject(value) &&
    nullableCount(value.cpu_permille) &&
    nullableCount(value.gpu_permille) &&
    nullableCount(value.memory_bytes) &&
    nullableCount(value.gpu_memory_bytes) &&
    nullableCount(value.fps) &&
    typeof value.released === "boolean"
  ) {
    return {
      cpu_permille: value.cpu_permille,
      gpu_permille: value.gpu_permille,
      memory_bytes: value.memory_bytes,
      gpu_memory_bytes: value.gpu_memory_bytes,
      fps: value.fps,
      released: value.released,
    };
  }
  throw new ProtocolError(`uso inválido: ${JSON.stringify(value)}`);
}

export function parseEvent(value: unknown): DaemonEvent {
  if (isObject(value)) {
    if (value.kind === "status_changed") {
      return { kind: "status_changed", status: parseStatus(value.status) };
    }
    if (value.kind === "playback_failed" && oneOf(errorCodes, value.error)) {
      return { kind: "playback_failed", error: value.error };
    }
    if (value.kind === "policy_changed") {
      return { kind: "policy_changed", policy: parsePolicy(value.policy) };
    }
  }
  throw new ProtocolError(`evento inválido: ${JSON.stringify(value)}`);
}

/** O código de um erro rejeitado por `invoke`. O que não for do app vira `internal`. */
export function commandErrorCode(error: unknown): CommandErrorCode {
  if (isObject(error)) {
    const { code } = error;
    if (code === "offline" || oneOf(errorCodes, code)) {
      return code;
    }
  }
  return "internal";
}
