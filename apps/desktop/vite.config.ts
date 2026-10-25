import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Porta fixa: o tauri.conf.json aponta o devUrl para ela.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
  },
});
