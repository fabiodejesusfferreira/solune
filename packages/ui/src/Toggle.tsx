export interface ToggleProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  /** A opção no afirmativo, como "Pausar na bateria". */
  label: string;
}

/** Interruptor para opções que valem na hora, sem botão de confirmar. */
export function Toggle({ checked, onChange, label }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      className="sl-toggle"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
    >
      <span className="sl-toggle__track" aria-hidden="true">
        <span className="sl-toggle__knob" />
      </span>
      {label}
    </button>
  );
}
