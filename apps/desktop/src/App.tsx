import { version } from "../package.json";
import { Logo } from "@solune/ui";
import { listen } from "@tauri-apps/api/event";
import { Gauge, LayoutGrid, Monitor, Moon, Sun } from "lucide-react";
import { type ReactNode, useEffect, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { useAppTheme } from "./appTheme";
import { onDaemonDisconnected, onDaemonEvent } from "./daemon/api";
import { useDaemon } from "./daemon/store";
import { Detail } from "./library/Detail";
import { Library } from "./library/Library";
import { Onboarding } from "./library/Onboarding";
import { fileName, useAllowMedia, useLibrary } from "./library/store";
import { Monitors } from "./monitors/Monitors";
import { Rules } from "./rules/Rules";
import { logicalCores, type UsageHistory, useUsage } from "./rules/usage";
import { Themes } from "./themes/Themes";

export const screens = ["library", "monitors", "themes", "rules"] as const;
export type Screen = (typeof screens)[number];

export function App() {
  useDaemonSync();
  useAllowMedia();
  const { t } = useTranslation();
  const scheme = useColorScheme();
  const { override, set: setTheme } = useAppTheme();
  const [screen, setScreen] = useScreen();
  const [detail, setDetail] = useState<string | null>(null);
  const empty = useLibrary((state) => state.items.length === 0);
  const usage = useUsage();

  useEffect(() => {
    if (override) {
      document.documentElement.dataset.theme = override;
    } else {
      delete document.documentElement.dataset.theme;
    }
  }, [override]);

  const go = (target: Screen) => {
    setDetail(null);
    setScreen(target);
  };

  if (screen === "library" && empty) {
    return <Onboarding />;
  }

  const item = (target: Screen, icon: ReactNode, label: string) => (
    <li>
      <button
        type="button"
        className="nav__item"
        aria-current={screen === target ? "page" : undefined}
        onClick={() => go(target)}
      >
        {icon}
        {label}
      </button>
    </li>
  );

  return (
    <div className="app">
      <aside className="app__sidebar">
        <div className="brand">
          <Logo variant="mark" on={scheme} size={32} />
          <span className="brand__name" aria-hidden="true">
            so<span className="brand__sun">l</span>une
          </span>
          <button
            type="button"
            role="switch"
            className="theme-switch"
            aria-checked={scheme === "dark"}
            aria-label={t("nav.dark_mode")}
            onClick={() => setTheme(scheme === "dark" ? "light" : "dark")}
          >
            <Sun size={12} aria-hidden="true" />
            <Moon size={12} aria-hidden="true" />
            <span className="theme-switch__knob" aria-hidden="true" />
          </button>
        </div>
        <nav aria-label={t("nav.label")}>
          <ul className="nav">
            {item("library", <LayoutGrid size={18} aria-hidden="true" />, t("nav.library"))}
            {item("monitors", <Monitor size={18} aria-hidden="true" />, t("nav.monitors"))}
            {item("themes", <Sun size={18} aria-hidden="true" />, t("nav.themes"))}
            {item("rules", <Gauge size={18} aria-hidden="true" />, t("nav.rules"))}
          </ul>
        </nav>
        <NowPlayingCard usage={usage} />
        <p className="app__version text-caption">{t("nav.version", { version })}</p>
      </aside>
      {screen === "library" && detail ? (
        <Detail path={detail} usage={usage} onBack={() => setDetail(null)} />
      ) : screen === "library" ? (
        <Library onOpen={setDetail} />
      ) : screen === "monitors" ? (
        <Monitors onBrowse={() => go("library")} />
      ) : screen === "themes" ? (
        <Themes />
      ) : (
        <Rules usage={usage} />
      )}
    </div>
  );
}

/** O que toca agora e quanto custa, no pé da barra lateral. */
function NowPlayingCard({ usage }: { usage: UsageHistory }) {
  const { t, i18n } = useTranslation();
  const status = useDaemon((state) => state.status);
  const name = useLibrary((state) => state.items.find((item) => item.path === status?.path)?.name);
  if (!status?.path || status.state === "stopped") {
    return null;
  }
  const u = usage.usage;
  const number = (value: number) =>
    value.toLocaleString(i18n.language, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const parts = [
    u?.cpu_permille != null ? `CPU ${number(u.cpu_permille / 10 / logicalCores())}%` : null,
    u?.gpu_permille != null ? `GPU ${number(u.gpu_permille / 10)}%` : null,
    u?.fps != null ? t("rules.usage.fps_value", { value: u.fps }) : null,
  ].filter(Boolean);
  return (
    <section className="now-card" aria-label={t("now.title")}>
      <span className="text-label now-card__muted">
        {status.state === "playing" ? t("now.title") : t("player.state.paused")}
      </span>
      <span className="text-body-strong now-card__name">{name ?? fileName(status.path)}</span>
      {parts.length > 0 ? (
        <span className="text-data now-card__muted">{parts.join(" · ")}</span>
      ) : null}
    </section>
  );
}

/** A tela aberta. A bandeja pode pedir outra pelo evento `app://navigate`. */
function useScreen() {
  const [screen, setScreen] = useState<Screen>("library");
  useEffect(() => {
    const unlisten = listen<Screen>("app://navigate", ({ payload }) => {
      if ((screens as readonly string[]).includes(payload)) {
        setScreen(payload);
      }
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);
  return [screen, setScreen] as const;
}

/** Status inicial, eventos do daemon e reconexão quando a conexão cai. */
function useDaemonSync() {
  const { refresh, apply } = useDaemon();
  useEffect(() => {
    void refresh();
    const subscriptions = [onDaemonEvent(apply), onDaemonDisconnected(() => void refresh())];
    return () => {
      for (const subscription of subscriptions) {
        void subscription.then((stop) => stop());
      }
    };
  }, [refresh, apply]);
}

const darkQuery = "(prefers-color-scheme: dark)";

export function useColorScheme(): "light" | "dark" {
  const override = useAppTheme((state) => state.override);
  const system = useSystemDark();
  return override ?? (system ? "dark" : "light");
}

function useSystemDark(): boolean {
  return useSyncExternalStore(
    (onChange) => {
      const query = window.matchMedia(darkQuery);
      query.addEventListener("change", onChange);
      return () => query.removeEventListener("change", onChange);
    },
    () => window.matchMedia(darkQuery).matches,
  );
}
