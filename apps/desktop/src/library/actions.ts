import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { daemon, mediaList } from "../daemon/api";
import { useDaemon } from "../daemon/store";
import { mediaExtensions, slideFor, useLibrary } from "./store";

/** Abre o seletor de arquivos (ou de pasta) e põe o que veio na biblioteca. */
export async function importFiles(filterName: string, directory = false): Promise<number> {
  const picked = await open(
    directory
      ? { multiple: true, directory: true }
      : { multiple: true, filters: [{ name: filterName, extensions: mediaExtensions }] },
  );
  if (!picked) {
    return 0;
  }
  return useLibrary.getState().add(await mediaList([picked].flat()));
}

/** Aplica um wallpaper só (com o par claro e escuro, se tiver) em todas as telas. */
export function applyWallpaper(path: string) {
  const { items, setCarousel } = useLibrary.getState();
  return useDaemon.getState().run(async () => {
    await daemon.playList({ items: [slideFor(items, path)], every_minutes: 0, shuffle: false });
    setCarousel({ on: false });
  });
}

/** Aplica o carrossel como está na biblioteca. */
export function applyCarousel() {
  const { items, carousel, setCarousel } = useLibrary.getState();
  return useDaemon.getState().run(async () => {
    await daemon.playList({
      items: carousel.paths.map((path) => slideFor(items, path)),
      every_minutes: carousel.every_minutes,
      shuffle: carousel.shuffle,
    });
    setCarousel({ on: true });
  });
}

/** Arquivos soltos na janela entram na biblioteca. Devolve se há algo sendo arrastado. */
export function useDropZone(): boolean {
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") {
        setDragging(true);
      } else {
        setDragging(false);
        if (payload.type === "drop") {
          void mediaList(payload.paths).then(useLibrary.getState().add, () => {});
        }
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  return dragging;
}
