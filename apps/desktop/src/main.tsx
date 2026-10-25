import "@solune/ui/fonts";
import "@solune/ui/styles.css";
import "./app.css";
import { createI18n } from "@solune/i18n";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import { App } from "./App";

const root = document.getElementById("root");
if (!root) {
  throw new Error("index.html sem #root");
}
const i18n = await createI18n();
createRoot(root).render(
  <StrictMode>
    <I18nextProvider i18n={i18n}>
      <App />
    </I18nextProvider>
  </StrictMode>,
);
