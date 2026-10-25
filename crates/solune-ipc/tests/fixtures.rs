//! Exemplos do protocolo em JSON, que a interface usa para testar os tipos
//! TypeScript escritos à mão (`apps/desktop/src/daemon/protocol.test.ts`).
//!
//! O teste falha quando a serialização muda. Para regravar os arquivos de
//! propósito: `SOLUNE_UPDATE_FIXTURES=1 cargo test -p solune-ipc --test fixtures`
//! (ou `pnpm ipc:fixtures`).

use std::path::PathBuf;

use serde_json::{Value, json};
use solune_ipc::{ErrorCode, Event, PauseReason, PlayerState, Status};

fn fixtures() -> Vec<(&'static str, Value)> {
    let playing = Status {
        state: PlayerState::Playing,
        path: Some("/home/ana/Vídeos/praia.mp4".into()),
        pause_reason: None,
        hwdec: Some("vaapi".into()),
    };
    let paused = Status {
        state: PlayerState::Paused,
        pause_reason: Some(PauseReason::Battery),
        ..playing.clone()
    };
    vec![
        (
            "status",
            json!({
                "stopped": to_value(&Status::default()),
                "playing": to_value(&playing),
                "paused": to_value(&paused),
            }),
        ),
        (
            "events",
            json!([
                to_value(&Event::StatusChanged { status: paused }),
                to_value(&Event::PlaybackFailed {
                    error: ErrorCode::UnsupportedFormat,
                }),
            ]),
        ),
        (
            "codes",
            json!({
                "error": [
                    ErrorCode::FileNotFound,
                    ErrorCode::UnsupportedFormat,
                    ErrorCode::SurfaceUnavailable,
                    ErrorCode::Incompatible,
                    ErrorCode::Internal,
                ],
                "pause_reason": [
                    PauseReason::User,
                    PauseReason::Fullscreen,
                    PauseReason::Battery,
                    PauseReason::Locked,
                ],
            }),
        ),
    ]
}

fn to_value(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("serializa")
}

#[test]
fn fixtures_batem_com_o_protocolo() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let update = std::env::var_os("SOLUNE_UPDATE_FIXTURES").is_some();
    for (name, value) in fixtures() {
        let path = dir.join(format!("{name}.json"));
        let expected = serde_json::to_string_pretty(&value).unwrap() + "\n";
        if update {
            std::fs::write(&path, &expected).unwrap();
            continue;
        }
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; rode pnpm ipc:fixtures", path.display()));
        assert_eq!(
            actual,
            expected,
            "{} mudou; se foi de propósito, rode pnpm ipc:fixtures e ajuste os tipos TS",
            path.display()
        );
    }
}
