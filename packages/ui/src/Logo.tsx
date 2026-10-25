import markOnDark from "../assets/solune-mark-on-dark.svg";
import markOnLight from "../assets/solune-mark-on-light.svg";
import symbolOnDark from "../assets/solune-symbol-on-dark.svg";
import symbolOnLight from "../assets/solune-symbol-on-light.svg";

const files = {
  mark: { light: markOnLight, dark: markOnDark },
  symbol: { light: symbolOnLight, dark: symbolOnDark },
} as const;

export interface LogoProps {
  /** mark: a marca completa, a partir de 32px. symbol: o símbolo reduzido, abaixo disso. */
  variant?: "mark" | "symbol";
  /** O fundo em que o logo fica. O arquivo é escolhido por ele, nunca recolorido. */
  on: "light" | "dark";
  size: number;
}

export function Logo({ variant = "mark", on, size }: LogoProps) {
  return <img src={files[variant][on]} width={size} height={size} alt="Solune" />;
}
