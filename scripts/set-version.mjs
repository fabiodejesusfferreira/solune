// Troca a versão do Solune em todos os arquivos que a guardam e atualiza o
// Cargo.lock. Uso: pnpm version:set 0.2.0   (aceita 1.0.0-rc.1)
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const version = process.argv[2];
const semver = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;
if (!version || !semver.test(version)) {
  console.error("uso: pnpm version:set <MAJOR.MINOR.PATCH[-pré-lançamento]>");
  process.exit(1);
}

// Cada arquivo e o trecho que carrega a versão (a primeira ocorrência).
const files = {
  "crates/solune-core/Cargo.toml": /^version = ".*"$/m,
  "crates/solune-ipc/Cargo.toml": /^version = ".*"$/m,
  "crates/solune-daemon/Cargo.toml": /^version = ".*"$/m,
  "apps/desktop/src-tauri/Cargo.toml": /^version = ".*"$/m,
  "apps/desktop/src-tauri/tauri.conf.json": /^ {2}"version": ".*"/m,
  "apps/desktop/package.json": /^ {2}"version": ".*"/m,
  "packages/tokens/package.json": /^ {2}"version": ".*"/m,
  "packages/i18n/package.json": /^ {2}"version": ".*"/m,
  "packages/ui/package.json": /^ {2}"version": ".*"/m,
};

for (const [file, pattern] of Object.entries(files)) {
  const path = join(root, file);
  const text = readFileSync(path, "utf8");
  if (!pattern.test(text)) {
    throw new Error(`não achei a versão em ${file}`);
  }
  // Preserva a vírgula final do JSON.
  const next = text.replace(pattern, (m) =>
    file.endsWith(".toml")
      ? `version = "${version}"`
      : `  "version": "${version}"${m.endsWith(",") ? "," : ""}`,
  );
  writeFileSync(path, next);
  console.log(`${file} -> ${version}`);
}

execFileSync("cargo", ["update", "--workspace"], { cwd: root, stdio: "inherit" });
console.log(`
Versão ${version} pronta. Falta:
  1. escrever a seção ${version} no CHANGELOG.md
  2. git commit -am "Versão ${version}"
  3. git tag v${version} && git push origin main v${version}`);
