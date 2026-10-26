import { useEffect } from "react";
import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";
import { mediaAllow, mediaThumb } from "../daemon/api";
import type { Playlist, Theme } from "../daemon/protocol";

/** Extensões que a biblioteca aceita, no diálogo e no arrastar e soltar. */
export const videoExtensions = ["mp4", "webm", "mkv", "mov", "m4v"] as const;
export const imageExtensions = ["png", "jpg", "jpeg", "webp"] as const;
export const mediaExtensions = [...videoExtensions, ...imageExtensions];

export type Kind = "video" | "image";

export interface LibraryItem {
  path: string;
  name: string;
  kind: Kind;
  /** O tema para o qual o wallpaper foi escolhido. */
  theme?: Theme | undefined;
  /** O wallpaper do outro tema, formando um par. */
  pair?: string | undefined;
  /** Em bytes. */
  size?: number | undefined;
  width?: number | undefined;
  height?: number | undefined;
  /** Em segundos, só vídeo. */
  duration?: number | undefined;
}

export interface Carousel {
  paths: string[];
  /** 0 troca a cada login. */
  every_minutes: number;
  shuffle: boolean;
  /** O carrossel é o que está aplicado agora. */
  on: boolean;
}

interface LibraryState {
  items: LibraryItem[];
  carousel: Carousel;
  /** Arquivos já liberados para o protocolo `asset:` nesta sessão. Não é salvo. */
  allowed: string[];
  /** Miniatura JPEG de cada vídeo, pelo caminho do vídeo. Não é salvo. */
  thumbs: Record<string, string>;
  /** Acrescenta os arquivos novos. Devolve quantos entraram. */
  add: (paths: readonly string[]) => number;
  remove: (path: string) => void;
  update: (path: string, patch: Partial<LibraryItem>) => void;
  /** Marca o tema do wallpaper; o par, se houver, fica com o outro. */
  setTheme: (path: string, theme: Theme | undefined) => void;
  /** Junta dois wallpapers num par claro e escuro. `undefined` desfaz o par. */
  setPair: (path: string, other: string | undefined) => void;
  setCarousel: (patch: Partial<Carousel>) => void;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function kindOf(path: string): Kind | null {
  const extension = path.split(".").pop()?.toLowerCase() ?? "";
  if ((videoExtensions as readonly string[]).includes(extension)) {
    return "video";
  }
  return (imageExtensions as readonly string[]).includes(extension) ? "image" : null;
}

const opposite = (theme: Theme): Theme => (theme === "light" ? "dark" : "light");

/** O item da lista do daemon para um wallpaper: com o par, o claro e o escuro. */
export function slideFor(items: LibraryItem[], path: string): Playlist["items"][number] {
  const item = items.find((i) => i.path === path);
  const pair = item?.pair ? items.find((i) => i.path === item.pair) : undefined;
  if (!item || !pair) {
    return { path, dark: null };
  }
  return item.theme === "dark"
    ? { path: pair.path, dark: item.path }
    : { path: item.path, dark: pair.path };
}

// Provisório: a biblioteca fica no armazenamento da webview até o app ter
// banco de dados com miniaturas e metadados.
export const useLibrary = create<LibraryState>()(
  persist(
    (set, get) => {
      const patch = (paths: Record<string, Partial<LibraryItem>>) =>
        set((state) => ({
          items: state.items.map((item) =>
            paths[item.path] ? { ...item, ...paths[item.path] } : item,
          ),
        }));
      return {
        items: [],
        carousel: { paths: [], every_minutes: 30, shuffle: false, on: false },
        allowed: [],
        thumbs: {},
        add: (paths) => {
          const known = new Set(get().items.map((item) => item.path));
          const fresh: LibraryItem[] = [];
          for (const path of new Set(paths)) {
            const kind = kindOf(path);
            if (kind && !known.has(path)) {
              fresh.push({ path, name: fileName(path), kind });
            }
          }
          if (fresh.length > 0) {
            set((state) => ({ items: [...state.items, ...fresh] }));
          }
          return fresh.length;
        },
        remove: (path) => {
          const item = get().items.find((i) => i.path === path);
          set((state) => ({
            items: state.items
              .filter((i) => i.path !== path)
              .map((i) => (i.path === item?.pair ? { ...i, pair: undefined } : i)),
            carousel: {
              ...state.carousel,
              paths: state.carousel.paths.filter((p) => p !== path),
            },
          }));
        },
        update: (path, change) => patch({ [path]: change }),
        setTheme: (path, theme) => {
          const item = get().items.find((i) => i.path === path);
          patch({
            [path]: { theme },
            ...(item?.pair && theme ? { [item.pair]: { theme: opposite(theme) } } : {}),
          });
        },
        setPair: (path, other) => {
          const items = get().items;
          const item = items.find((i) => i.path === path);
          if (!item) {
            return;
          }
          const theme = item.theme ?? "dark";
          const old = [item.pair, items.find((i) => i.path === other)?.pair];
          patch({
            ...Object.fromEntries(old.filter(Boolean).map((p) => [p, { pair: undefined }])),
            [path]: { theme, pair: other },
            ...(other ? { [other]: { theme: opposite(theme), pair: path } } : {}),
          });
        },
        setCarousel: (change) => set((state) => ({ carousel: { ...state.carousel, ...change } })),
      };
    },
    {
      name: "solune.library",
      version: 2,
      storage: createJSONStorage(() => localStorage),
      partialize: ({ items, carousel }) => ({ items, carousel }),
      // A versão 1 guardava só vídeos, com caminho e nome.
      migrate: (persisted, version) => {
        const state = persisted as { items?: LibraryItem[] };
        if (version < 2) {
          state.items = (state.items ?? []).map((item) => ({ ...item, kind: "video" }));
        }
        return state as LibraryState;
      },
    },
  ),
);

/**
 * Libera os arquivos da biblioteca para as miniaturas e guarda o tamanho de
 * cada um. O escopo do protocolo `asset:` zera quando o app reinicia.
 */
export function useAllowMedia() {
  const items = useLibrary((state) => state.items);
  useEffect(() => {
    const pending = items.map((item) => item.path).filter((path) => !requested.has(path));
    if (pending.length === 0) {
      return;
    }
    for (const path of pending) {
      requested.add(path);
    }
    mediaAllow(pending).then(
      (sizes) => {
        const { allowed, update } = useLibrary.getState();
        // A miniatura só pede o arquivo depois de liberado; antes, o asset: recusa.
        useLibrary.setState({ allowed: [...allowed, ...pending] });
        sizes.forEach((size, i) => {
          if (size !== null) {
            update(pending[i] ?? "", { size });
          }
        });
      },
      () => {
        for (const path of pending) {
          requested.delete(path);
        }
      },
    );
    // Uma por vez: cada miniatura abre um ffmpeg.
    void (async () => {
      for (const path of pending) {
        if (kindOf(path) !== "video") {
          continue;
        }
        const thumb = await mediaThumb(path).catch(() => null);
        if (thumb) {
          const { thumbs, update } = useLibrary.getState();
          useLibrary.setState({ thumbs: { ...thumbs, [path]: thumb.path } });
          update(path, { width: thumb.width, height: thumb.height, duration: thumb.duration });
        }
      }
    })();
  }, [items]);
}

const requested = new Set<string>();
