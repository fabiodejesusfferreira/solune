import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { useCallback, useEffect, useState } from "react";

export interface Autostart {
  /** `null` enquanto o app ainda não sabe. */
  enabled: boolean | null;
  failed: boolean;
  set: (value: boolean) => Promise<void>;
}

/** Iniciar com o sistema, pelo plugin de autostart do Tauri. */
export function useAutostart(): Autostart {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let alive = true;
    isEnabled().then(
      (value) => alive && setEnabled(value),
      () => alive && setFailed(true),
    );
    return () => {
      alive = false;
    };
  }, []);

  const set = useCallback(async (value: boolean) => {
    setFailed(false);
    try {
      await (value ? enable() : disable());
      setEnabled(await isEnabled());
    } catch {
      setFailed(true);
    }
  }, []);

  return { enabled, failed, set };
}
