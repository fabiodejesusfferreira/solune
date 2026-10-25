import { fireEvent, render, screen } from "@testing-library/react";
import { Button } from "./Button";
import { Chip } from "./Chip";
import { Toggle } from "./Toggle";
import { WallpaperCard } from "./WallpaperCard";

describe("Button", () => {
  it("é do tipo button por padrão, para não enviar formulários", () => {
    render(<Button>Importar vídeo</Button>);
    expect(screen.getByRole("button", { name: "Importar vídeo" })).toHaveAttribute(
      "type",
      "button",
    );
  });

  it("aplica a variante e o tamanho", () => {
    render(
      <Button variant="sun" size="sm">
        Definir como papel de parede
      </Button>,
    );
    expect(screen.getByRole("button")).toHaveClass("sl-btn--sun", "sl-btn--sm");
  });
});

describe("Toggle", () => {
  it("expõe o estado como switch e pede o valor oposto ao clicar", () => {
    const onChange = vi.fn();
    render(<Toggle checked={false} onChange={onChange} label="Pausar na bateria" />);
    const toggle = screen.getByRole("switch", { name: "Pausar na bateria" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);
    expect(onChange).toHaveBeenCalledWith(true);
  });
});

describe("Chip", () => {
  it("mostra o ícone de estado escondido do leitor de tela, ao lado da palavra", () => {
    render(
      <Chip tone="success" icon={<svg />}>
        Aplicado
      </Chip>,
    );
    const chip = screen.getByText("Aplicado");
    expect(chip).toHaveClass("sl-chip--success");
    expect(chip.querySelector("[aria-hidden='true']")).not.toBeNull();
  });
});

describe("WallpaperCard", () => {
  it("marca o wallpaper ativo com aria-pressed e mostra o selo", () => {
    const onSelect = vi.fn();
    render(
      <WallpaperCard
        title="Praia ao entardecer"
        badge="0:24"
        active
        art={<svg />}
        onSelect={onSelect}
      />,
    );
    const card = screen.getByRole("button", { name: /Praia ao entardecer/ });
    expect(card).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("0:24")).toBeInTheDocument();
    fireEvent.click(card);
    expect(onSelect).toHaveBeenCalledOnce();
  });
});
