import type { ReactNode } from "react";

export interface WallpaperCardProps {
  title: string;
  /** Resolução e tamanho, em ink-muted. */
  meta?: string;
  /** Miniatura (SVG ou img) que preenche a área de 124px de altura. */
  art: ReactNode;
  /** Duração ou resolução, como "0:24" ou "4K". Um selo por cartão. */
  badge?: string;
  /** Wallpaper em uso: borda sun, sem mudar o fundo. */
  active?: boolean;
  onSelect?: () => void;
}

export function WallpaperCard({
  title,
  meta,
  art,
  badge,
  active = false,
  onSelect,
}: WallpaperCardProps) {
  return (
    <button type="button" className="sl-card" aria-pressed={active} onClick={onSelect}>
      <span className="sl-card__art">
        {art}
        {badge ? <span className="sl-card__badge">{badge}</span> : null}
      </span>
      <span className="sl-card__body">
        <span className="sl-card__title">{title}</span>
        {meta ? <span className="sl-card__meta">{meta}</span> : null}
      </span>
    </button>
  );
}
