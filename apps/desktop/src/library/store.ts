import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/** Extensões que a biblioteca aceita, no diálogo e no arrastar e soltar. */
export const videoExtensions = ["mp4", "webm", "mkv", "mov", "m4v"] as const;

export interface LibraryItem {
  path: string;
  name: string;
}

interface LibraryState {
  items: LibraryItem[];
  selected: string | null;
  /** Acrescenta os vídeos novos e seleciona o primeiro. Devolve quantos entraram. */
  add: (paths: readonly string[]) => number;
  select: (path: string) => void;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function isVideo(path: string): boolean {
  const extension = path.split(".").pop()?.toLowerCase() ?? "";
  return (videoExtensions as readonly string[]).includes(extension);
}

// Provisório: a biblioteca fica no armazenamento da webview até o app ter
// banco de dados com miniaturas e metadados.
export const useLibrary = create<LibraryState>()(
  persist(
    (set, get) => ({
      items: [],
      selected: null,
      add: (paths) => {
        const known = new Set(get().items.map((item) => item.path));
        const fresh = [...new Set(paths)]
          .filter((path) => isVideo(path) && !known.has(path))
          .map((path) => ({ path, name: fileName(path) }));
        const first = fresh[0];
        if (first) {
          set((state) => ({ items: [...state.items, ...fresh], selected: first.path }));
        }
        return fresh.length;
      },
      select: (path) => set({ selected: path }),
    }),
    { name: "solune.library", version: 1, storage: createJSONStorage(() => localStorage) },
  ),
);
