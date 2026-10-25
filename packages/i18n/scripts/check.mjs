// Checa o catálogo pt-BR contra o código e contra o tom de voz:
// - chave usada no código que não existe no pt-BR;
// - chave do pt-BR que nenhum código usa;
// - exclamação e palavras que o design system proíbe.

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { flatten, readCatalog } from "./catalog.mjs";

const root = new URL("../../..", import.meta.url).pathname;
const catalog = new Map(flatten(readCatalog("pt-BR")));

// Chaves que chegam como código do daemon (pause_reason.fullscreen,
// error.file_not_found) e são montadas em tempo de execução.
const dynamicPrefixes = ["pause_reason.", "error."];
const pluralSuffix = /_(zero|one|two|few|many|other)$/;
const forbidden = ["simplesmente", "fácil", "incrível", "com sucesso"];

const scanRoots = ["apps", "packages/ui"];
const skipDirs = new Set(["node_modules", "dist", "target", "gen"]);
const exts = new Set([".ts", ".tsx", ".rs"]);
// t("chave"), i18n.t("chave") e, no Rust, t!("chave") ou tr("chave").
const usage = /\b(?:t!?|tr)\(\s*["'`]([a-z0-9_]+(?:\.[a-z0-9_]+)+)["'`]/g;

function* walk(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (!skipDirs.has(entry.name)) yield* walk(path);
    } else if (exts.has(extname(entry.name)) && !entry.name.includes(".test.")) {
      yield path;
    }
  }
}

const used = new Map();
for (const base of scanRoots) {
  for (const file of walk(join(root, base))) {
    for (const match of readFileSync(file, "utf8").matchAll(usage)) {
      used.set(match[1], relative(root, file));
    }
  }
}

const problems = [];
const baseKey = (key) => key.replace(pluralSuffix, "");
const catalogBases = new Set([...catalog.keys()].map(baseKey));

for (const [key, file] of used) {
  if (!catalogBases.has(key)) problems.push(`chave usada sem texto no pt-BR: ${key} (${file})`);
}
for (const key of catalog.keys()) {
  const dynamic = dynamicPrefixes.some((p) => key.startsWith(p));
  if (!dynamic && !used.has(baseKey(key))) problems.push(`chave sem uso no código: ${key}`);
}
for (const [key, text] of catalog) {
  if (text.includes("!")) problems.push(`exclamação em ${key}: ${text}`);
  for (const word of forbidden) {
    if (text.toLowerCase().includes(word)) problems.push(`"${word}" em ${key}: ${text}`);
  }
}

if (problems.length > 0) {
  for (const p of problems) console.error(p);
  process.exit(1);
}
console.log(`i18n: ${catalog.size} textos, ${used.size} chaves usadas no código`);
