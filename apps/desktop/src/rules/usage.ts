import { useEffect, useState } from "react";
import { type AppMemory, appMemory, daemon } from "../daemon/api";
import type { Usage } from "../daemon/protocol";

/** Uma leitura por segundo, e o gráfico guarda o último minuto. */
export const usageInterval = 1000;
export const historySize = 60;

export interface UsageHistory {
  usage: Usage | null;
  /** Memória do daemon, do app e da webview; `null` se o sistema não informa. */
  memory: AppMemory | null;
  /** CPU de cada leitura, em milésimos de um núcleo, da mais antiga à mais nova. */
  cpu: number[];
}

/** Soma das partes que existem agora; `null` se nenhuma foi medida. */
export function totalMemory(memory: AppMemory | null): number | null {
  const parts = [memory?.daemon, memory?.app, memory?.webview, memory?.other];
  return parts.reduce<number | null>((sum, part) => (part == null ? sum : (sum ?? 0) + part), null);
}

/**
 * Lê o uso do daemon e a memória do app enquanto a tela está montada e a
 * janela está visível. Escondido na bandeja, o app não pergunta nada: medir também custa.
 */
export function useUsage(): UsageHistory {
  const [state, setState] = useState<UsageHistory>({ usage: null, memory: null, cpu: [] });

  useEffect(() => {
    let alive = true;
    async function read() {
      if (document.visibilityState !== "visible") {
        return;
      }
      let usage: Usage | null = null;
      try {
        usage = await daemon.usage();
      } catch {
        // Sem conexão, o aviso vem do store; a próxima leitura tenta de novo.
      }
      // Sem o daemon, o app e a webview continuam medidos.
      const memory = await appMemory(usage?.pid ?? null).catch(() => null);
      if (alive) {
        setState(({ cpu }) => ({
          usage,
          memory,
          cpu: usage?.cpu_permille == null ? cpu : [...cpu, usage.cpu_permille].slice(-historySize),
        }));
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
