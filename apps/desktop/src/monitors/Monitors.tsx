import { Button, Toggle } from "@solune/ui";
import { availableMonitors, type Monitor, primaryMonitor } from "@tauri-apps/api/window";
import { ArrowDown, ArrowUp, SkipForward } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { daemon } from "../daemon/api";
import { ConnectionNotice } from "../daemon/ConnectionNotice";
import { useDaemon } from "../daemon/store";
import { applyCarousel } from "../library/actions";
import { badgeFor, Thumb } from "../library/media";
import { useLibrary } from "../library/store";

const intervals = [5, 30, 60, 0] as const;

export function Monitors({ onBrowse }: { onBrowse: () => void }) {
  const { t } = useTranslation();
  const carouselOn = useLibrary((state) => state.carousel.on);
  const error = useDaemon((state) => state.error);
  const [mode, setMode] = useState<"single" | "carousel">(carouselOn ? "carousel" : "single");

  return (
    <main className="page">
      <header className="stack-2">
        <h1 className="text-title">{t("monitors.title")}</h1>
        <p className="text-body muted measure">{t("monitors.intro")}</p>
      </header>
      <ConnectionNotice />
      {error ? (
        <p role="alert" className="alert-text text-caption">
          {t(`error.${error}`)}
        </p>
      ) : null}

      <Screens />

      <section className="stack-5" aria-labelledby="config-title">
        <div className="page__bar">
          <h2 id="config-title" className="text-heading">
            {t("monitors.all")}
          </h2>
          <fieldset className="chips" aria-label={t("monitors.mode")}>
            <Button
              size="sm"
              variant={mode === "single" ? "primary" : "secondary"}
              onClick={() => setMode("single")}
            >
              {t("monitors.single")}
            </Button>
            <Button
              size="sm"
              variant={mode === "carousel" ? "primary" : "secondary"}
              onClick={() => setMode("carousel")}
            >
              {t("monitors.carousel")}
            </Button>
          </fieldset>
        </div>
        {mode === "carousel" ? <CarouselEditor /> : <Single onBrowse={onBrowse} />}
      </section>
    </main>
  );
}

/** As telas do sistema, em escala, com o wallpaper que toca nelas. */
function Screens() {
  const { t } = useTranslation();
  const [monitors, setMonitors] = useState<{ list: Monitor[]; primary: string | null }>({
    list: [],
    primary: null,
  });
  const status = useDaemon((state) => state.status);
  const item = useLibrary((state) => state.items.find((i) => i.path === status?.path));
  useEffect(() => {
    Promise.all([availableMonitors(), primaryMonitor()]).then(
      ([list, primary]) => setMonitors({ list, primary: primary?.name ?? null }),
      () => {},
    );
  }, []);
  if (monitors.list.length === 0) {
    return null;
  }
  const widest = Math.max(...monitors.list.map((m) => m.size.width));
  return (
    <section className="screens panel" aria-label={t("monitors.yours")}>
      {monitors.list.map((monitor, i) => {
        const width = Math.round((220 * monitor.size.width) / widest);
        const height = Math.round((width * monitor.size.height) / monitor.size.width);
        const primary = monitor.name === monitors.primary;
        return (
          <figure key={monitor.name ?? i} className="screens__item">
            <span className="screens__art" style={{ width, height }}>
              {status?.state !== "stopped" ? <Thumb item={item} /> : null}
            </span>
            <figcaption className="stack-0">
              <span className="text-body-strong">{t("monitors.name", { n: i + 1 })}</span>
              <span className="text-data muted">
                {monitor.size.width} x {monitor.size.height}
                {primary ? ` · ${t("monitors.primary")}` : ""}
              </span>
            </figcaption>
          </figure>
        );
      })}
    </section>
  );
}

function Single({ onBrowse }: { onBrowse: () => void }) {
  const { t } = useTranslation();
  const status = useDaemon((state) => state.status);
  const item = useLibrary((state) => state.items.find((i) => i.path === status?.path));
  return (
    <div className="now panel">
      <div className="now__art">
        <Thumb item={item} />
      </div>
      <div className="now__info">
        <span className="text-body-strong">{item?.name ?? t("monitors.nothing")}</span>
        {item ? (
          <span className="text-caption muted">
            {[item.kind === "video" ? t("detail.video") : t("detail.image"), badgeFor(item)]
              .filter(Boolean)
              .join(" · ")}
          </span>
        ) : null}
      </div>
      <Button onClick={onBrowse}>{t("monitors.change")}</Button>
    </div>
  );
}

/** A lista do carrossel e de quanto em quanto tempo ela troca. */
function CarouselEditor() {
  const { t } = useTranslation();
  const { items, carousel, setCarousel } = useLibrary();
  const { connection, run, status } = useDaemon();
  const addId = useId();
  const list = carousel.paths
    .map((path) => items.find((i) => i.path === path))
    .filter((i) => i !== undefined);
  const rest = items.filter((i) => !carousel.paths.includes(i.path));
  const move = (from: number, to: number) => {
    const paths = [...carousel.paths];
    const [path] = paths.splice(from, 1);
    if (path) {
      paths.splice(to, 0, path);
      setCarousel({ paths });
    }
  };
  const label: Record<(typeof intervals)[number], string> = {
    5: t("monitors.every.5"),
    30: t("monitors.every.30"),
    60: t("monitors.every.60"),
    0: t("monitors.every.login"),
  };

  return (
    <div className="split">
      <div className="split__main stack-2">
        {list.length === 0 ? <p className="text-body muted">{t("monitors.empty")}</p> : null}
        <ol className="playlist">
          {list.map((item, i) => (
            <li key={item.path} className="playlist__row">
              <span className="playlist__art">
                <Thumb item={item} />
              </span>
              <span className="playlist__info">
                <span className="text-body-strong">{item.name}</span>
                <span className="text-caption muted">
                  {[item.kind === "video" ? t("detail.video") : t("detail.image"), badgeFor(item)]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
              </span>
              <Button
                variant="ghost"
                size="sm"
                aria-label={t("monitors.up", { name: item.name })}
                disabled={i === 0}
                onClick={() => move(i, i - 1)}
              >
                <ArrowUp size={16} aria-hidden="true" />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                aria-label={t("monitors.down", { name: item.name })}
                disabled={i === list.length - 1}
                onClick={() => move(i, i + 1)}
              >
                <ArrowDown size={16} aria-hidden="true" />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                aria-label={t("monitors.remove_named", { name: item.name })}
                onClick={() =>
                  setCarousel({ paths: carousel.paths.filter((p) => p !== item.path) })
                }
              >
                {t("monitors.remove")}
              </Button>
            </li>
          ))}
        </ol>
        {rest.length > 0 ? (
          <div className="add-row">
            <label htmlFor={addId} className="text-label muted">
              {t("monitors.add")}
            </label>
            <select
              id={addId}
              className="field field--select"
              value=""
              onChange={(e) =>
                e.target.value && setCarousel({ paths: [...carousel.paths, e.target.value] })
              }
            >
              <option value="">{t("monitors.add_pick")}</option>
              {rest.map((i) => (
                <option key={i.path} value={i.path}>
                  {i.name}
                </option>
              ))}
            </select>
          </div>
        ) : null}
      </div>

      <div className="split__side panel panel--padded stack-4">
        <h3 className="text-heading">{t("monitors.interval")}</h3>
        <fieldset className="chips" aria-label={t("monitors.interval")}>
          {intervals.map((minutes) => (
            <Button
              key={minutes}
              size="sm"
              variant={carousel.every_minutes === minutes ? "primary" : "secondary"}
              onClick={() => setCarousel({ every_minutes: minutes })}
            >
              {label[minutes]}
            </Button>
          ))}
        </fieldset>
        <Toggle
          checked={carousel.shuffle}
          onChange={(shuffle) => setCarousel({ shuffle })}
          label={t("monitors.shuffle")}
        />
        <div className="stack-2">
          <Button
            variant="sun"
            disabled={list.length === 0 || connection === "incompatible"}
            onClick={() => void applyCarousel()}
          >
            {carousel.on ? t("monitors.apply_again") : t("monitors.apply")}
          </Button>
          {carousel.on && status?.state !== "stopped" ? (
            <Button
              icon={<SkipForward size={16} />}
              disabled={connection !== "online"}
              onClick={() => run(daemon.next)}
            >
              {t("monitors.next")}
            </Button>
          ) : null}
        </div>
        <p className="text-caption muted">{t("monitors.same_everywhere")}</p>
      </div>
    </div>
  );
}
