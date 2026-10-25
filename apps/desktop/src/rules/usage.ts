import { useEffect, useState } from "react";
import { daemon } from "../daemon/api";
import type { Usage } from "../daemon/protocol";

/** Uma leitura por segundo, e o gráfico guarda o último minuto. */
export const usageInterval = 1000;
export const historySize = 60;

export interface UsageHistory {
  usage: Usage | null;
  /** CPU de cada leitura, em milésimos de um núcleo, da mais antiga à mais nova. */
  cpu: number[];
}

/**
 * Lê o uso do daemon enquanto a tela está montada e a janela está visível.
 * Escondido na bandeja, o app não pergunta nada: medir também custa.
 */
export function useUsage(): UsageHistory {
  const [state, setState] = useState<UsageHistory>({ usage: null, cpu: [] });

  useEffect(() => {
    let alive = true;
    async function read() {
      if (document.visibilityState !== "visible") {
        return;
      }
      try {
        const usage = await daemon.usage();
        if (alive) {
          setState(({ cpu }) => ({
            usage,
            cpu:
              usage.cpu_permille === null ? cpu : [...cpu, usage.cpu_permille].slice(-historySize),
          }));
        }
      } catch {
        // Sem conexão, o aviso vem do store; a próxima leitura tenta de novo.
        if (alive) {
          setState((previous) => ({ ...previous, usage: null }));
        }
      }
    }
    void read();
    const timer = setInterval(read, usageInterval);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  return state;
}

/** Quantos núcleos lógicos a máquina tem, para mostrar a CPU como o monitor do sistema. */
export function logicalCores(): number {
  return Math.max(1, navigator.hardwareConcurrency || 1);
}
