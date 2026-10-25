import i18next, { type i18n } from "i18next";
import { initReactI18next } from "react-i18next";
import ptBR from "../locales/pt-BR.json" with { type: "json" };

/** Idioma-base: as chaves de verdade e o fallback de todas as outras línguas. */
export const baseLanguage = "pt-BR";

export const resources = {
  "pt-BR": { translation: ptBR },
} as const;

declare module "i18next" {
  interface CustomTypeOptions {
    resources: { translation: typeof ptBR };
  }
}

/**
 * Cria a instância do i18next para o app. Números e plurais passam pelo
 * Intl, que o i18next já usa por baixo.
 */
export async function createI18n(language: string = baseLanguage): Promise<i18n> {
  const instance = i18next.createInstance();
  await instance.use(initReactI18next).init({
    lng: language,
    fallbackLng: baseLanguage,
    resources,
    interpolation: { escapeValue: false },
    returnNull: false,
  });
  return instance;
}
