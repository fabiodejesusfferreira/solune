// Regrava os exemplos do protocolo em crates/solune-ipc/fixtures. Rode quando
// mudar o protocolo de propósito, e ajuste apps/desktop/src/daemon/protocol.ts.
import { execFileSync } from "node:child_process";

execFileSync("cargo", ["test", "-p", "solune-ipc", "--test", "fixtures"], {
  stdio: "inherit",
  env: { ...process.env, SOLUNE_UPDATE_FIXTURES: "1" },
});
