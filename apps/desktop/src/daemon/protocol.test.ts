import codes from "../../../../crates/solune-ipc/fixtures/codes.json";
import events from "../../../../crates/solune-ipc/fixtures/events.json";
import statuses from "../../../../crates/solune-ipc/fixtures/status.json";
import { commandErrorCode, errorCodes, parseEvent, parseStatus, pauseReasons } from "./protocol";

describe("contrato com o protocolo do Rust", () => {
  it("lê todo status que o daemon manda", () => {
    expect(parseStatus(statuses.stopped).state).toBe("stopped");
    expect(parseStatus(statuses.playing)).toEqual({
      state: "playing",
      path: "/home/ana/Vídeos/praia.mp4",
      pause_reason: null,
      hwdec: "vaapi",
    });
    expect(parseStatus(statuses.paused).pause_reason).toBe("battery");
  });

  it("lê todo evento que o daemon manda", () => {
    expect(events.map((event) => parseEvent(event).kind)).toEqual([
      "status_changed",
      "playback_failed",
    ]);
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
  });

  it("traduz erros do invoke para códigos", () => {
    expect(commandErrorCode({ code: "offline" })).toBe("offline");
    expect(commandErrorCode({ code: "file_not_found" })).toBe("file_not_found");
    expect(commandErrorCode(new Error("x"))).toBe("internal");
  });
});
