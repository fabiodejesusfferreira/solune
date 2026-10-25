import { Logo } from "@solune/ui";
import { listen } from "@tauri-apps/api/event";
import { Gauge, LayoutGrid } from "lucide-react";
import { type ReactNode, useEffect, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { onDaemonDisconnected, onDaemonEvent } from "./daemon/api";
import { useDaemon } from "./daemon/store";
import { Library } from "./library/Library";
import { Rules } from "./rules/Rules";

export type Screen = "library" | "rules";

export function App() {
  useDaemonSync();
  const { t } = useTranslation();
  const scheme = useColorScheme();
  const [screen, setScreen] = useScreen();

  const item = (target: Screen, icon: ReactNode, label: string) => (
    <li>
      <button
        type="button"
        className="nav__item"
        aria-current={screen === target ? "page" : undefined}
        onClick={() => setScreen(target)}
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
        </div>
        <nav aria-label={t("nav.label")}>
          <ul className="nav">
            {item("library", <LayoutGrid size={18} aria-hidden="true" />, t("nav.library"))}
            {item("rules", <Gauge size={18} aria-hidden="true" />, t("nav.rules"))}
          </ul>
        </nav>
      </aside>
      {screen === "library" ? <Library /> : <Rules />}
    </div>
  );
}

/** A tela aberta. A bandeja pode pedir outra pelo evento `app://navigate`. */
function useScreen() {
  const [screen, setScreen] = useState<Screen>("library");
  useEffect(() => {
    const unlisten = listen<Screen>("app://navigate", ({ payload }) => {
      if (payload === "library" || payload === "rules") {
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

function useColorScheme(): "light" | "dark" {
  const dark = useSyncExternalStore(
    (onChange) => {
      const query = window.matchMedia(darkQuery);
      query.addEventListener("change", onChange);
      return () => query.removeEventListener("change", onChange);
    },
    () => window.matchMedia(darkQuery).matches,
  );
  return dark ? "dark" : "light";
}
