export interface ToggleProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  /** A opção no afirmativo, como "Pausar na bateria". */
  label: string;
  /** Enquanto o valor não chegou ou não pode mudar. */
  disabled?: boolean;
  /** O id do texto que explica a opção, lido depois do nome. */
  describedBy?: string;
}

/** Interruptor para opções que valem na hora, sem botão de confirmar. */
export function Toggle({ checked, onChange, label, disabled, describedBy }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      className="sl-toggle"
      aria-checked={checked}
      aria-describedby={describedBy}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="sl-toggle__track" aria-hidden="true">
        <span className="sl-toggle__knob" />
      </span>
      {label}
    </button>
  );
}
