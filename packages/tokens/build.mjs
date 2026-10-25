// Gera dist/tokens.css e dist/tokens.ts a partir do tokens.json (DTCG).
//
// O CSS segue o sistema por padrão e aceita o tema forçado por
// data-theme="light" ou data-theme="dark" no elemento raiz.

import StyleDictionary from "style-dictionary";

const HEADER = "/* Gerado de packages/tokens/tokens.json pelo build.mjs. Não edite à mão. */";

/** Nome do token sem o grupo: color.surface-0 vira surface-0. */
StyleDictionary.registerTransform({
  name: "name/solune",
  type: "name",
  transform: (token) => token.path.at(-1),
});

const dark = (token) => token.$extensions?.solune?.dark;
const isThemed = (token) => dark(token) !== undefined;
const decl = (name, value) => `  --${name}: ${value};`;

/** Referência {font.font-display} vira var(--font-display). */
const toVar = (value) =>
  typeof value === "string" ? value.replace(/\{[^}]*\.([^.}]+)\}/g, "var(--$1)") : value;

function typography(token) {
  const v = token.original.$value;
  return `${v.fontWeight} ${v.fontSize}/${v.lineHeight} ${toVar(v.fontFamily)}`;
}

function cssValue(token) {
  switch (token.$type) {
    case "typography":
      return typography(token);
    case "cubicBezier":
      return `cubic-bezier(${token.$value.join(", ")})`;
    default:
      return token.$value;
  }
}

StyleDictionary.registerFormat({
  name: "solune/css",
  format: ({ dictionary }) => {
    const tokens = dictionary.allTokens;
    const themed = tokens.filter(isThemed);
    const plain = tokens.filter((t) => !isThemed(t));
    const light = themed.map((t) => decl(t.name, t.$value));
    const darkDecls = themed.map((t) => decl(t.name, dark(t)));
    const indent = (lines) => lines.map((l) => `  ${l}`);

    const tracking = plain
      .filter((t) => t.$type === "typography" && t.original.$value.letterSpacing)
      .map((t) => decl(`${t.name}-tracking`, t.original.$value.letterSpacing));

    const classes = plain
      .filter((t) => t.$type === "typography")
      .map((t) => {
        const ls = t.original.$value.letterSpacing
          ? ` letter-spacing: var(--${t.name}-tracking);`
          : "";
        return `.${t.name} { font: var(--${t.name});${ls} }`;
      });

    return [
      HEADER,
      "",
      ":root,",
      ':root[data-theme="light"] {',
      "  color-scheme: light;",
      ...light,
      "}",
      "",
      "@media (prefers-color-scheme: dark) {",
      '  :root:not([data-theme="light"]) {',
      "    color-scheme: dark;",
      ...indent(darkDecls),
      "  }",
      "}",
      "",
      ':root[data-theme="dark"] {',
      "  color-scheme: dark;",
      ...darkDecls,
      "}",
      "",
      ":root {",
      ...plain.map((t) => decl(t.name, cssValue(t))),
      ...tracking,
      "}",
      "",
      ...classes,
      "",
    ].join("\n");
  },
});

StyleDictionary.registerFormat({
  name: "solune/ts",
  format: ({ dictionary }) => {
    const names = dictionary.allTokens.flatMap((t) =>
      t.$type === "typography" && t.original.$value.letterSpacing
        ? [t.name, `${t.name}-tracking`]
        : [t.name],
    );
    const colors = dictionary.allTokens.filter((t) => t.$type === "color").map((t) => t.name);
    const list = (xs) => xs.map((n) => `  "${n}",`).join("\n");
    return `${HEADER.replace("/*", "//").replace(" */", "")}

/** Todos os tokens que viram variável CSS. */
export const tokenNames = [
${list(names)}
] as const;

export type TokenName = (typeof tokenNames)[number];

export const colorNames = [
${list(colors)}
] as const;

export type ColorName = (typeof colorNames)[number];

/** \`var(--nome)\` de um token que existe. */
export function cssVar(name: TokenName): string {
  return \`var(--\${name})\`;
}
`;
  },
});

const sd = new StyleDictionary({
  source: ["tokens.json"],
  usesDtcg: true,
  log: { verbosity: "default", warnings: "error" },
  platforms: {
    web: {
      transforms: ["name/solune"],
      buildPath: "dist/",
      files: [
        { destination: "tokens.css", format: "solune/css" },
        { destination: "tokens.ts", format: "solune/ts" },
      ],
    },
  },
});

await sd.buildAllPlatforms();
