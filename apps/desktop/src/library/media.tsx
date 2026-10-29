import { convertFileSrc } from "@tauri-apps/api/core";
import { Film, Image } from "lucide-react";
import { type LibraryItem, useLibrary } from "./store";

/** Miniatura de um wallpaper: um quadro do vídeo (gerado no app), ou a imagem. */
export function Thumb({ item, className }: { item: LibraryItem | undefined; className?: string }) {
  const allowed = useLibrary((state) => (item ? state.allowed[item.path] === true : false));
  const thumb = useLibrary((state) => (item ? state.thumbs[item.path] : undefined));
  const update = useLibrary((state) => state.update);
  const file = item?.kind === "video" ? thumb : allowed ? item?.path : undefined;
  if (!item || !file) {
    const Icon = item?.kind === "image" ? Image : Film;
    return (
      <span className={`thumb thumb--empty ${className ?? ""}`}>
        <Icon size={24} aria-hidden="true" />
      </span>
    );
  }
  return (
    <img
      className={`thumb ${className ?? ""}`}
      src={convertFileSrc(file)}
      alt=""
      loading="lazy"
      onLoad={
        item.kind === "video" || item.width !== undefined
          ? undefined
          : (e) =>
              update(item.path, {
                width: e.currentTarget.naturalWidth,
                height: e.currentTarget.naturalHeight,
              })
      }
    />
  );
}

export function formatDuration(seconds: number): string {
  const total = Math.round(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

export function formatResolution(item: LibraryItem): string | null {
  return item.width && item.height ? `${item.width} x ${item.height}` : null;
}

export function formatSize(bytes: number, language: string): string {
  return `${(bytes / 2 ** 20).toLocaleString(language, { maximumFractionDigits: bytes < 2 ** 20 * 10 ? 1 : 0 })} MB`;
}

/** O selo do cartão: a duração do vídeo, ou a classe da resolução da imagem. */
export function badgeFor(item: LibraryItem): string | undefined {
  if (item.kind === "video") {
    return item.duration ? formatDuration(item.duration) : undefined;
  }
  if (!item.width || !item.height) {
    return undefined;
  }
  return item.width >= 3840 ? "4K" : item.width >= 2560 ? "2K" : `${item.height}p`;
}

/** Resolução e tamanho, como "3840 x 2160 · 48 MB". */
export function metaFor(item: LibraryItem, language: string): string {
  return [formatResolution(item), item.size ? formatSize(item.size, language) : null]
    .filter(Boolean)
    .join(" · ");
}
