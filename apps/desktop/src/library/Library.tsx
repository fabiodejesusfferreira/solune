import { Button, Chip, WallpaperCard } from "@solune/ui";
import { Pause, Play, Plus, Square, Trash2 } from "lucide-react";
import { memo, useState } from "react";
import { useTranslation } from "react-i18next";
import { daemon } from "../daemon/api";
import { ConnectionNotice } from "../daemon/ConnectionNotice";
import { useDaemon } from "../daemon/store";
import { importFiles, useDropZone } from "./actions";
import { badgeFor, metaFor, Thumb } from "./media";
import { type LibraryItem, useLibrary } from "./store";

const filters = ["all", "video", "image", "light", "dark"] as const;
type Filter = (typeof filters)[number];

const matches = (item: LibraryItem, filter: Filter) =>
  filter === "all" || item.kind === filter || item.theme === filter;

// memo: o App atualiza a cada leitura de uso (1 s) e a grade não depende dela.
export const Library = memo(function Library({ onOpen }: { onOpen: (path: string) => void }) {
  const { t, i18n } = useTranslation();
  const items = useLibrary((state) => state.items);
  const remove = useLibrary((state) => state.remove);
  const status = useDaemon((state) => state.status);
  const error = useDaemon((state) => state.error);
  const dragging = useDropZone();
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");

  const needle = query.trim().toLocaleLowerCase(i18n.language);
  const shown = items.filter(
    (item) =>
      matches(item, filter) &&
      (!needle || item.name.toLocaleLowerCase(i18n.language).includes(needle)),
  );
  const playingPath = status && status.state !== "stopped" ? status.path : null;
  const label: Record<Filter, string> = {
    all: t("library.filters.all"),
    video: t("library.filters.video"),
    image: t("library.filters.image"),
    light: t("library.filters.light"),
    dark: t("library.filters.dark"),
  };

  return (
    <main className="page" data-dragging={dragging || undefined}>
      <header className="page__bar">
        <h1 className="text-title">{t("library.title")}</h1>
        <div className="page__tools">
          <input
            type="search"
            className="field"
            aria-label={t("library.search_label")}
            placeholder={t("library.search")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <Button
            variant="primary"
            icon={<Plus size={16} />}
            onClick={() => void importFiles(t("library.filter"))}
          >
            {t("library.import")}
          </Button>
        </div>
      </header>

      <ConnectionNotice />
      {error ? (
        <p role="alert" className="alert-text text-caption">
          {t(`error.${error}`)}
        </p>
      ) : null}

      <NowPlaying onOpen={onOpen} />

      <fieldset className="filters" aria-label={t("library.filters.label")}>
        {filters.map((id) => (
          <button
            key={id}
            type="button"
            className="filters__chip"
            aria-pressed={filter === id}
            onClick={() => setFilter(id)}
          >
            <Chip tone={filter === id ? "sun" : "neutral"}>{label[id]}</Chip>
          </button>
        ))}
        <span className="filters__count text-caption">
          {t("library.shown", { shown: shown.length, count: items.length })}
        </span>
      </fieldset>

      <ul className="grid">
        {shown.map((item) => (
          <li key={item.path} className="grid__item">
            <WallpaperCard
              title={item.name}
              meta={metaFor(item, i18n.language)}
              badge={badgeFor(item) ?? ""}
              art={<Thumb item={item} />}
              active={item.path === playingPath}
              onSelect={() => onOpen(item.path)}
            />
            <button
              type="button"
              className="grid__remove"
              aria-label={t("library.delete")}
              onClick={() => remove(item.path)}
            >
              <Trash2 size={16} aria-hidden="true" />
            </button>
          </li>
        ))}
      </ul>

      {dragging ? (
        <div className="drop text-heading" aria-hidden="true">
          {t("library.drop")}
        </div>
      ) : null}
    </main>
  );
});

/** O wallpaper em uso, com os controles do player. */
function NowPlaying({ onOpen }: { onOpen: (path: string) => void }) {
  const { t, i18n } = useTranslation();
  const { status, connection, run } = useDaemon();
  const item = useLibrary((state) => state.items.find((i) => i.path === status?.path));
  const carousel = useLibrary((state) => state.carousel.on);
  if (!status || status.state === "stopped" || !status.path) {
    return null;
  }
  const online = connection === "online";
  const playing = status.state === "playing";
  return (
    <section className="now panel" aria-label={t("now.title")}>
      <div className="now__art">
        <Thumb item={item} />
      </div>
      <div className="now__info">
        <div className="chips">
          <Chip tone={playing ? "sun" : "neutral"} icon={playing ? "●" : "Ⅱ"}>
            {playing ? t("player.state.playing") : t("player.state.paused")}
          </Chip>
          <Chip>{t("monitors.all")}</Chip>
          {carousel ? <Chip>{t("monitors.carousel")}</Chip> : null}
        </div>
        <h2 className="text-heading">{item?.name ?? status.path}</h2>
        <p className="text-body muted">
          {!playing && status.pause_reason
            ? t(`pause_reason.${status.pause_reason}`)
            : item
              ? metaFor(item, i18n.language)
              : null}
        </p>
      </div>
      <div className="now__actions">
        {playing ? (
          <Button
            variant="primary"
            icon={<Pause size={16} />}
            disabled={!online}
            onClick={() => run(daemon.pause)}
          >
            {t("player.pause")}
          </Button>
        ) : (
          <Button
            variant="primary"
            icon={<Play size={16} />}
            disabled={!online}
            onClick={() => run(daemon.resume)}
          >
            {t("player.resume")}
          </Button>
        )}
        {item ? (
          <Button variant="ghost" onClick={() => onOpen(item.path)}>
            {t("library.details")}
          </Button>
        ) : null}
        <Button
          variant="ghost"
          icon={<Square size={16} />}
          disabled={!online}
          onClick={() => run(daemon.stop)}
        >
          {t("player.stop")}
        </Button>
      </div>
    </section>
  );
}
