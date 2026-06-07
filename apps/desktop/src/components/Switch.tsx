// iOS-style toggle switch (on-state matches the mockups' amber accent).

type SwitchProps = {
  on: boolean;
  disabled?: boolean;
  onChange: () => void;
  label?: string;
};

export function Switch({ on, disabled = false, onChange, label }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      className={on ? "switch switch--on" : "switch"}
      disabled={disabled}
      onClick={onChange}
    >
      <span className="switch-thumb" />
    </button>
  );
}
