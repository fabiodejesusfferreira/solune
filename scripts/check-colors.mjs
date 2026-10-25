// Barra cor literal (hexadecimal, rgb(), hsl()) fora do pacote de tokens.
// A interface usa sempre var(--nome); só packages/tokens tem valor de cor.
// SVGs ficam de fora: o logo tem as cores fixas no arquivo, de propósito.

import { readdirSync, readFileSync } from "node:fs";
import { extname, join, relative } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const roots = ["apps", "packages"];
const skipDirs = new Set(["node_modules", "dist", "target", "gen", "tokens"]);
const exts = new Set([".ts", ".tsx", ".css", ".html"]);
const literal = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?|oklch)\(/g;

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
    } else if (exts.has(extname(entry.name))) {
      yield path;
    }
  }
}

const found = [];
for (const base of roots) {
  for (const file of walk(join(root, base))) {
    readFileSync(file, "utf8")
      .split("\n")
      .forEach((line, i) => {
        for (const match of line.matchAll(literal)) {
          found.push(`${relative(root, file)}:${i + 1}: ${match[0]}`);
        }
      });
  }
}

if (found.length > 0) {
  console.error("Cor literal fora de packages/tokens. Use var(--nome) de um token:");
  for (const f of found) console.error(`  ${f}`);
  process.exit(1);
}
console.log("cores: nenhuma cor literal fora dos tokens");
