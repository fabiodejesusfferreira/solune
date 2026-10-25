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

export interface Status {
  state: PlayerState;
  path: string | null;
  pause_reason: PauseReason | null;
  /** Decodificação por hardware em uso, como o mpv informa. */
  hwdec: string | null;
}

export type DaemonEvent =
  | { kind: "status_changed"; status: Status }
  | { kind: "playback_failed"; error: ErrorCode };

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

export function parseStatus(value: unknown): Status {
  if (
    isObject(value) &&
    oneOf(playerStates, value.state) &&
    nullableString(value.path) &&
    (value.pause_reason === null || oneOf(pauseReasons, value.pause_reason)) &&
    nullableString(value.hwdec)
  ) {
    return {
      state: value.state,
      path: value.path,
      pause_reason: value.pause_reason,
      hwdec: value.hwdec,
    };
  }
  throw new ProtocolError(`status inválido: ${JSON.stringify(value)}`);
}

export function parseEvent(value: unknown): DaemonEvent {
  if (isObject(value)) {
    if (value.kind === "status_changed") {
      return { kind: "status_changed", status: parseStatus(value.status) };
    }
    if (value.kind === "playback_failed" && oneOf(errorCodes, value.error)) {
      return { kind: "playback_failed", error: value.error };
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
