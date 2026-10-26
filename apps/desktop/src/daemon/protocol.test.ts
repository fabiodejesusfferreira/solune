import codes from "../../../../crates/solune-ipc/fixtures/codes.json";
import events from "../../../../crates/solune-ipc/fixtures/events.json";
import policy from "../../../../crates/solune-ipc/fixtures/policy.json";
import statuses from "../../../../crates/solune-ipc/fixtures/status.json";
import usage from "../../../../crates/solune-ipc/fixtures/usage.json";
import {
  commandErrorCode,
  errorCodes,
  parseEvent,
  parsePolicy,
  parseStatus,
  parseUsage,
  pauseReasons,
  settingNames,
} from "./protocol";

describe("contrato com o protocolo do Rust", () => {
  it("lê todo status que o daemon manda", () => {
    expect(parseStatus(statuses.stopped).state).toBe("stopped");
    expect(parseStatus(statuses.playing)).toEqual({
      state: "playing",
      path: "/home/ana/Vídeos/praia.mp4",
      pause_reason: null,
      hwdec: "vaapi",
      theme: "dark",
    });
    expect(parseStatus(statuses.paused).pause_reason).toBe("battery");
  });

  it("lê todo evento que o daemon manda", () => {
    expect(events.map((event) => parseEvent(event).kind)).toEqual([
      "status_changed",
      "playback_failed",
      "policy_changed",
    ]);
  });

  it("lê a política e tem as mesmas preferências do Rust", () => {
    expect(parsePolicy(policy)).toEqual({
      settings: {
        pause_on_fullscreen: true,
        pause_on_battery: false,
        pause_on_lock: true,
        release_when_paused: true,
        sound: false,
        show_desktop_icons: true,
        schedule: { mode: "times", light_at: 420, dark_at: 1170 },
      },
      conditions: { fullscreen: true, on_battery: null, screen_locked: false, dark: true },
    });
    expect(Object.keys(policy.settings).sort()).toEqual([...settingNames, "schedule"].sort());
  });

  it("lê o uso, com e sem números", () => {
    expect(parseUsage(usage.playing)).toEqual({
      cpu_permille: 49,
      gpu_permille: 106,
      memory_bytes: 165216256,
      gpu_memory_bytes: 48234496,
      fps: 30,
      released: false,
    });
    expect(parseUsage(usage.unknown).cpu_permille).toBeNull();
  });

  it("tem os mesmos códigos de erro e de pausa do Rust", () => {
    expect(codes.error).toEqual([...errorCodes]);
    expect(codes.pause_reason).toEqual([...pauseReasons]);
  });

  it("recusa o que não é do protocolo", () => {
    expect(() => parseStatus({ state: "tocando" })).toThrow("status inválido");
    expect(() => parseEvent({ kind: "playback_failed", error: "outro" })).toThrow(
      "evento inválido",
    );
    expect(() => parsePolicy({ settings: {}, conditions: {} })).toThrow("política inválida");
    expect(() => parseUsage({ ...usage.playing, cpu_permille: -1 })).toThrow("uso inválido");
  });

  it("traduz erros do invoke para códigos", () => {
    expect(commandErrorCode({ code: "offline" })).toBe("offline");
    expect(commandErrorCode({ code: "file_not_found" })).toBe("file_not_found");
    expect(commandErrorCode(new Error("x"))).toBe("internal");
  });
});
