import { readFileSync } from "node:fs";

/** Lê um catálogo e devolve as chaves achatadas com seus textos. */
export function flatten(obj, prefix = "") {
  return Object.entries(obj).flatMap(([key, value]) =>
    typeof value === "string" ? [[`${prefix}${key}`, value]] : flatten(value, `${prefix}${key}.`),
  );
}

export function readCatalog(language) {
  const url = new URL(`../locales/${language}.json`, import.meta.url);
  return JSON.parse(readFileSync(url, "utf8"));
}
