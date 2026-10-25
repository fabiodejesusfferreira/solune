// Recalcula o contraste dos pares que o design system promete, nos dois
// temas, a partir do tokens.json. Falha se algum ficar abaixo do mínimo:
// 4,5:1 para texto, 3:1 para bordas de controle, foco e texto grande.

import { readFileSync } from "node:fs";

const tokens = JSON.parse(readFileSync(new URL("../tokens.json", import.meta.url), "utf8"));
const color = tokens.color;

const surfaces = ["surface-0", "surface-1", "surface-2"];
const TEXT = 4.5;
const UI = 3;

/** [frente, fundo, mínimo] */
const pairs = [
  ...["ink", "ink-muted", "sun-text", "success", "danger"].flatMap((fg) =>
    surfaces.map((bg) => [fg, bg, TEXT]),
  ),
  ["on-primary", "primary", TEXT],
  ["on-sun", "sun", TEXT],
  ["line-strong", "surface-0", UI],
  ["line-strong", "surface-1", UI],
  ...surfaces.map((bg) => ["focus", bg, UI]),
];

function rgb(hex) {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!m) throw new Error(`cor sem suporte no cálculo de contraste: ${hex}`);
  const n = Number.parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function luminance(hex) {
  const [r, g, b] = rgb(hex).map((c) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function ratio(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const value = (name, theme) => {
  const token = color[name];
  if (!token) throw new Error(`token de cor inexistente: ${name}`);
  return theme === "dark" ? token.$extensions.solune.dark : token.$value;
};

let failures = 0;
for (const theme of ["light", "dark"]) {
  for (const [fg, bg, min] of pairs) {
    const r = ratio(value(fg, theme), value(bg, theme));
    if (r < min) {
      failures += 1;
      console.error(`${theme}: ${fg} sobre ${bg} tem ${r.toFixed(2)}:1, mínimo ${min}:1`);
    }
  }
}

if (failures > 0) {
  console.error(`${failures} par(es) abaixo do mínimo`);
  process.exit(1);
}
console.log(`contraste: ${pairs.length * 2} pares conferidos nos dois temas`);
