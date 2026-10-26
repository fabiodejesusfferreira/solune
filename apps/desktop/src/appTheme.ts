import { create } from "zustand";
import { createJSONStorage, persist } from "zustand/middleware";

/** O tema do app quando o usuário não segue o do sistema. `null` segue o sistema. */
export const useAppTheme = create<{
  override: "light" | "dark" | null;
  set: (theme: "light" | "dark") => void;
}>()(
  persist((set) => ({ override: null, set: (override) => set({ override }) }), {
    name: "solune.appTheme",
    storage: createJSONStorage(() => localStorage),
    partialize: ({ override }) => ({ override }),
  }),
);
