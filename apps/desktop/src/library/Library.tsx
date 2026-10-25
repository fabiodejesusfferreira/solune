import { Button, Chip, WallpaperCard } from "@solune/ui";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { Film, Pause, Play, Plus, Square, Upload } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { daemon } from "../daemon/api";
import { ConnectionNotice } from "../daemon/ConnectionNotice";
import { useDaemon } from "../daemon/store";
import { useLibrary, videoExtensions } from "./store";

export function Library() {
  const { t } = useTranslation();
  const { items, selected, add, select } = useLibrary();
  const { status, run } = useDaemon();
  const dragging = useDropZone(add);

  async function importVideos() {
    const paths = await open({
      multiple: true,
      filters: [{ name: t("library.filter"), extensions: [...videoExtensions] }],
    });
    if (paths) {
      add(paths);
    }
  }

  const playingPath = status && status.state !== "stopped" ? status.path : null;

  return (
    <main className="library" data-dragging={dragging || undefined}>
      <header className="library__header">
        <div>
          <h1 className="text-title">{t("library.title")}</h1>
          {items.length > 0 ? (
            <p className="library__count text-caption">
              {t("library.count", { count: items.length })}
            </p>
          ) : null}
        </div>
        <Button variant="primary" icon={<Plus size={16} />} onClick={importVideos}>
          {t("library.import")}
        </Button>
      </header>

      {items.length === 0 ? (
        <div className="library__empty">
          <Upload size={32} aria-hidden="true" />
          <p className="text-body">{t("library.empty")}</p>
        </div>
      ) : (
        <ul className="library__grid">
          {items.map((item) => (
            <li key={item.path}>
              <WallpaperCard
                title={item.name}
                meta={item.name.split(".").pop()?.toUpperCase() ?? ""}
                art={<Film size={32} aria-hidden="true" className="library__art" />}
                active={item.path === playingPath}
                onSelect={() => select(item.path)}
              />
            </li>
          ))}
        </ul>
      )}

      {dragging ? (
        <div className="library__drop text-heading" aria-hidden="true">
          {t("library.drop")}
        </div>
      ) : null}

      <PlayerBar
        selected={selected}
        onApply={(path) => run(() => daemon.play(path))}
        onPause={() => run(daemon.pause)}
        onResume={() => run(daemon.resume)}
        onStop={() => run(daemon.stop)}
      />
    </main>
  );
}

interface PlayerBarProps {
  selected: string | null;
  onApply: (path: string) => void;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
}

function PlayerBar({ selected, onApply, onPause, onResume, onStop }: PlayerBarProps) {
  const { t } = useTranslation();
  const { connection, status, error } = useDaemon();
  const state = status?.state ?? "stopped";
  const online = connection === "online";

  return (
    <footer className="player">
      <div className="player__status">
        {connection === "online" ? (
          <Chip tone={state === "playing" ? "sun" : "neutral"}>
            {state === "playing"
              ? t("player.state.playing")
              : state === "paused"
                ? t("player.state.paused")
                : t("player.state.stopped")}
          </Chip>
        ) : null}
        {state === "paused" && status?.pause_reason ? (
          <span className="text-caption">{t(`pause_reason.${status.pause_reason}`)}</span>
        ) : null}
        <ConnectionNotice />
        {error ? (
          <p role="alert" className="alert-text text-caption">
            {t(`error.${error}`)}
          </p>
        ) : null}
      </div>
      <div className="player__actions">
        {state === "playing" ? (
          <Button icon={<Pause size={16} />} disabled={!online} onClick={onPause}>
            {t("player.pause")}
          </Button>
        ) : null}
        {state === "paused" ? (
          <Button icon={<Play size={16} />} disabled={!online} onClick={onResume}>
            {t("player.resume")}
          </Button>
        ) : null}
        {state !== "stopped" ? (
          <Button variant="ghost" icon={<Square size={16} />} disabled={!online} onClick={onStop}>
            {t("player.stop")}
          </Button>
        ) : null}
        <Button
          variant="sun"
          disabled={selected === null || connection === "incompatible"}
          onClick={() => selected && onApply(selected)}
        >
          {t("player.apply")}
        </Button>
      </div>
    </footer>
  );
}

/** Arquivos soltos na janela entram na biblioteca. Devolve se há algo sendo arrastado. */
function useDropZone(add: (paths: readonly string[]) => number): boolean {
  const [dragging, setDragging] = useState(false);
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") {
        setDragging(true);
      } else {
        setDragging(false);
        if (payload.type === "drop") {
          add(payload.paths);
        }
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [add]);
  return dragging;
}
