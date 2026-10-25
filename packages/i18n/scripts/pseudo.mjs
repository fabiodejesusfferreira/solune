// Gera dist/pseudo.json a partir do pt-BR: cada texto fica 30% mais longo
// e com letras acentuadas, para ver nas telas onde inglês, espanhol e
// alemão vão estourar. Interpolações como {{count}} ficam intactas.

import { mkdirSync, writeFileSync } from "node:fs";
import { readCatalog } from "./catalog.mjs";

const accents = { a: "å", e: "ë", i: "ï", o: "ø", u: "ü", A: "Å", E: "Ë", I: "Ï", O: "Ø", U: "Ü" };

function pseudo(text) {
  const parts = text.split(/(\{\{[^}]+\}\})/);
  const body = parts
    .map((part) =>
      part.startsWith("{{") ? part : part.replace(/[aeiouAEIOU]/g, (c) => accents[c]),
    )
    .join("");
  const pad = "·".repeat(Math.ceil(text.length * 0.3));
  return `[${body}${pad}]`;
}

function walk(obj) {
  return Object.fromEntries(
    Object.entries(obj).map(([k, v]) => [k, typeof v === "string" ? pseudo(v) : walk(v)]),
  );
}

const out = new URL("../dist/", import.meta.url);
mkdirSync(out, { recursive: true });
writeFileSync(
  new URL("pseudo.json", out),
  `${JSON.stringify(walk(readCatalog("pt-BR")), null, 2)}\n`,
);
console.log("pseudo-locale gerado em dist/pseudo.json");
