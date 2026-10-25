import { Logo } from "@solune/ui";
import { useEffect, useSyncExternalStore } from "react";
import { onDaemonDisconnected, onDaemonEvent } from "./daemon/api";
import { useDaemon } from "./daemon/store";
import { Library } from "./library/Library";

export function App() {
  useDaemonSync();
  const scheme = useColorScheme();
  return (
    <div className="app">
      <aside className="app__rail">
        <Logo variant="symbol" on={scheme} size={28} />
      </aside>
      <Library />
    </div>
  );
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
