import { Button, Chip, Toggle } from "@solune/ui";
import { convertFileSrc } from "@tauri-apps/api/core";
import { ChevronLeft } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConnectionNotice } from "../daemon/ConnectionNotice";
import type { Theme } from "../daemon/protocol";
import { useDaemon } from "../daemon/store";
import { logicalCores, type UsageHistory } from "../rules/usage";
import { applyWallpaper } from "./actions";
import { formatDuration, formatResolution, formatSize, Thumb } from "./media";
import { useLibrary } from "./store";

interface DetailProps {
  path: string;
  usage: UsageHistory;
  onBack: () => void;
}

export function Detail({ path, usage, onBack }: DetailProps) {
  const { t, i18n } = useTranslation();
  const { items, remove, setTheme, setPair } = useLibrary();
  const { status, policy, connection, error, setSetting } = useDaemon();
  const item = items.find((i) => i.path === path);
  const pairId = useId();
  if (!item) {
    return null;
  }
  const playing = status?.state !== "stopped" && status?.path === item.path;
  const pair = items.find((i) => i.path === item.pair);
  const other: Theme | undefined = item.theme
    ? item.theme === "light"
      ? "dark"
      : "light"
    : undefined;
  const themeLabel = { light: t("themes.light"), dark: t("themes.dark") };

  return (
    <main className="page">
      <button type="button" className="back text-body" onClick={onBack}>
        <ChevronLeft size={16} aria-hidden="true" />
        {t("detail.back")}
      </button>

      <header className="stack-2">
        <h1 className="text-title">{item.name}</h1>
        <div className="chips">
          <Chip>{item.kind === "video" ? t("detail.video") : t("detail.image")}</Chip>
          {formatResolution(item) ? <Chip>{formatResolution(item)}</Chip> : null}
          {item.duration ? <Chip>{formatDuration(item.duration)}</Chip> : null}
          {item.size ? <Chip>{formatSize(item.size, i18n.language)}</Chip> : null}
          {playing ? (
            <Chip tone="success" icon="✓">
              {t("detail.applied")}
            </Chip>
          ) : null}
        </div>
        <p className="detail__path text-data muted">{item.path}</p>
      </header>

      <ConnectionNotice />
      {error ? (
        <p role="alert" className="alert-text text-caption">
          {t(`error.${error}`)}
        </p>
      ) : null}

      <div className="split">
        <section className="split__main stack-4" aria-label={t("detail.preview")}>
          <Preview path={item.path} kind={item.kind} />
          {item.kind === "video" ? (
            <Toggle
              checked={policy?.settings.sound ?? false}
              disabled={!policy || connection !== "online"}
              onChange={(value) => void setSetting("sound", value)}
              label={t("detail.sound")}
            />
          ) : null}
        </section>

        <aside className="split__side panel panel--padded stack-5" aria-label={t("detail.apply")}>
          <div className="stack-3">
            <h2 className="text-heading">{t("detail.apply_on")}</h2>
            <div className="chips">
              <Chip>{t("monitors.all")}</Chip>
            </div>
          </div>

          <div className="stack-3">
            <h2 className="text-heading">{t("detail.theme")}</h2>
            <p className="text-body muted">{t("detail.theme_help")}</p>
            <fieldset className="chips" aria-label={t("detail.theme")}>
              {(["light", "dark"] as const).map((theme) => (
                <Button
                  key={theme}
                  size="sm"
                  variant={item.theme === theme ? "primary" : "secondary"}
                  onClick={() => setTheme(item.path, item.theme === theme ? undefined : theme)}
                >
                  {themeLabel[theme]}
                </Button>
              ))}
            </fieldset>
            {other ? (
              <div className="pair">
                <span className="pair__art">
                  <Thumb item={pair} />
                </span>
                <div className="pair__info">
                  <label htmlFor={pairId} className="text-label muted">
                    {other === "light" ? t("detail.pair_light") : t("detail.pair_dark")}
                  </label>
                  <select
                    id={pairId}
                    className="field field--select"
                    value={item.pair ?? ""}
                    onChange={(e) => setPair(item.path, e.target.value || undefined)}
                  >
                    <option value="">{t("detail.no_pair")}</option>
                    {items
                      .filter((i) => i.path !== item.path)
                      .map((i) => (
                        <option key={i.path} value={i.path}>
                          {i.name}
                        </option>
                      ))}
                  </select>
                </div>
              </div>
            ) : null}
          </div>

          {playing ? <Cost usage={usage} hwdec={status?.hwdec ?? null} /> : null}

          <div className="stack-2">
            <Button
              variant="sun"
              disabled={connection === "incompatible"}
              onClick={() => void applyWallpaper(item.path)}
            >
              {playing ? t("detail.apply_again") : t("player.apply")}
            </Button>
            <Button
              variant="ghost"
              onClick={() => {
                remove(item.path);
                onBack();
              }}
            >
              {t("detail.remove")}
            </Button>
          </div>
        </aside>
      </div>
    </main>
  );
}

/** Prévia em tamanho grande. O vídeo toca mudo aqui; o som é do wallpaper. */
function Preview({ path, kind }: { path: string; kind: "video" | "image" }) {
  const allowed = useLibrary((state) => state.allowed[path] === true);
  const [time, setTime] = useState({ at: 0, total: 0 });
  const item = useLibrary((state) => state.items.find((i) => i.path === path));
  const src = useVideoSrc(path, kind === "video" && allowed, item?.size);
  if (!src) {
    return (
      <div className="preview">
        <Thumb item={item} />
      </div>
    );
  }
  const progress = time.total ? (time.at / time.total) * 100 : 0;
  return (
    <div className="preview">
      <video
        src={src}
        autoPlay
        loop
        muted
        playsInline
        onTimeUpdate={(e) =>
          setTime({ at: e.currentTarget.currentTime, total: e.currentTarget.duration || 0 })
        }
      />
      <div className="preview__bar">
        <span className="text-data">
          {formatDuration(time.at)} / {formatDuration(time.total)}
        </span>
        <span className="preview__track">
          <span className="preview__fill" style={{ width: `${progress}%` }} />
        </span>
        {item && formatResolution(item) ? (
          <span className="text-data">{formatResolution(item)}</span>
        ) : null}
      </div>
    </div>
  );
}

/** Acima disso, a prévia fica na miniatura em vez de carregar o vídeo todo na memória. */
const maxPreviewBytes = 512 * 2 ** 20;

/**
 * O vídeo como `blob:`. O WebKitGTK não toca `<video>` direto do protocolo
 * `asset:`, mas toca de um blob. `null` enquanto carrega, se falhar ou se o
 * arquivo for grande demais: a prévia mostra a miniatura.
 */
function useVideoSrc(path: string, enabled: boolean, size: number | undefined): string | null {
  const [src, setSrc] = useState<string | null>(null);
  const tooBig = (size ?? 0) > maxPreviewBytes;
  useEffect(() => {
    if (!enabled || tooBig) {
      return;
    }
    let url: string | undefined;
    let cancelled = false;
    fetch(convertFileSrc(path))
      .then((response) => response.blob())
      .then((blob) => {
        if (!cancelled) {
          url = URL.createObjectURL(blob);
          setSrc(url);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      setSrc(null);
      if (url) {
        URL.revokeObjectURL(url);
      }
    };
  }, [path, enabled, tooBig]);
  return src;
}

/** O custo do wallpaper agora, com o mesmo número da tela de desempenho. */
function Cost({ usage, hwdec }: { usage: UsageHistory; hwdec: string | null }) {
  const { t, i18n } = useTranslation();
  const u = usage.usage;
  // A memória do player, medida como na tela de desempenho.
  const ram = usage.memory?.daemon ?? u?.memory_bytes;
  const number = (value: number) =>
    value.toLocaleString(i18n.language, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const parts = [
    u?.cpu_permille != null ? `CPU ${number(u.cpu_permille / 10 / logicalCores())}%` : null,
    u?.gpu_permille != null ? `GPU ${number(u.gpu_permille / 10)}%` : null,
    ram != null ? t("detail.ram", { value: Math.round(ram / 2 ** 20) }) : null,
  ].filter(Boolean);
  return (
    <div className="inset stack-2">
      <span className="text-label muted">{t("detail.cost")}</span>
      <span className="text-data">{parts.length > 0 ? parts.join(" · ") : "—"}</span>
      {hwdec ? (
        <span className="text-caption muted">
          {hwdec === "no" ? t("detail.decode_cpu") : t("detail.decode_gpu", { hwdec })}
        </span>
      ) : null}
    </div>
  );
}
