// Compila o daemon em release e copia para onde o Tauri procura o sidecar:
// apps/desktop/src-tauri/binaries/solune-daemon-<alvo>[.exe].
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit" });

const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (\S+)$/m)?.[1];
if (!host) {
  throw new Error("não achei o alvo do rustc em `rustc -vV`");
}
const exe = process.platform === "win32" ? ".exe" : "";

run("cargo", ["build", "--release", "-p", "solune-daemon"]);
const target = join(root, "apps/desktop/src-tauri/binaries", `solune-daemon-${host}${exe}`);
mkdirSync(dirname(target), { recursive: true });
copyFileSync(join(root, "target/release", `solune-daemon${exe}`), target);
console.log(`daemon pronto em ${target}`);
