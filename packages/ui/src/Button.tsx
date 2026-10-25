import type { ReactNode } from "react";
import { cx } from "./cx";

export interface ButtonProps {
  /** primary: ação principal da tela. sun: no máximo um por tela, para tocar ou aplicar. */
  variant?: "primary" | "secondary" | "ghost" | "sun";
  size?: "md" | "sm";
  /** Ícone de 16px antes do rótulo. */
  icon?: ReactNode;
  type?: "button" | "submit";
  disabled?: boolean;
  onClick?: () => void;
  "aria-label"?: string;
  /** Rótulo: verbo no início, de 1 a 3 palavras, sem ponto final. */
  children: ReactNode;
}

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  type = "button",
  disabled,
  onClick,
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cx("sl-btn", `sl-btn--${variant}`, size === "sm" && "sl-btn--sm")}
      disabled={disabled}
      onClick={onClick}
      aria-label={rest["aria-label"]}
    >
      {icon ? (
        <span className="sl-btn__icon" aria-hidden="true">
          {icon}
        </span>
      ) : null}
      {children}
    </button>
  );
}
