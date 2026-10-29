// Lê as exportações de uma DLL e escreve o .def, para o `lib /def` gerar a
// biblioteca de importação que o linker do MSVC pede. O libmpv para Windows
// só vem com a .dll.a do MinGW.
// Uso: node scripts/mpv-def.mjs libmpv-2.dll mpv.def
import { readFileSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const [dll, out] = process.argv.slice(2);
if (!dll || !out) {
  console.error("uso: node scripts/mpv-def.mjs <dll> <saida.def>");
  process.exit(1);
}
const buf = readFileSync(dll);
const pe = buf.readUInt32LE(0x3c);
if (buf.toString("latin1", pe, pe + 4) !== "PE\0\0" || buf.readUInt16LE(pe + 24) !== 0x20b) {
  throw new Error("não é uma DLL PE32+ (64 bits)");
}
const sections = Array.from({ length: buf.readUInt16LE(pe + 6) }, (_, i) => {
  const at = pe + 24 + buf.readUInt16LE(pe + 20) + i * 40;
  return {
    rva: buf.readUInt32LE(at + 12),
    size: buf.readUInt32LE(at + 8),
    raw: buf.readUInt32LE(at + 20),
  };
});
const offset = (rva) => {
  const s = sections.find((s) => rva >= s.rva && rva < s.rva + s.size);
  if (!s) {
    throw new Error(`RVA ${rva} fora das seções`);
  }
  return rva - s.rva + s.raw;
};
const exports = offset(buf.readUInt32LE(pe + 24 + 112));
const count = buf.readUInt32LE(exports + 24);
const names = offset(buf.readUInt32LE(exports + 32));
const symbols = Array.from({ length: count }, (_, i) => {
  const start = offset(buf.readUInt32LE(names + i * 4));
  return buf.toString("latin1", start, buf.indexOf(0, start));
});
if (!symbols.includes("mpv_create")) {
  throw new Error("a DLL não exporta mpv_create");
}
writeFileSync(out, `LIBRARY ${basename(dll)}\nEXPORTS\n${symbols.join("\n")}\n`);
console.log(`${symbols.length} símbolos em ${out}`);
