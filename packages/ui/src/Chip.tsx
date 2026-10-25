import type { ReactNode } from "react";
import { cx } from "./cx";

export interface ChipProps {
  /** neutral para categorias, sun para o que está ativo, success e danger para resultado. */
  tone?: "neutral" | "sun" | "success" | "danger";
  /** Ícone de estado. Em success e danger é obrigatório: a cor sozinha não basta. */
  icon?: ReactNode;
  children: ReactNode;
}

export function Chip({ tone = "neutral", icon, children }: ChipProps) {
  return (
    <span className={cx("sl-chip", tone !== "neutral" && `sl-chip--${tone}`)}>
      {icon ? <span aria-hidden="true">{icon}</span> : null}
      {children}
    </span>
  );
}
