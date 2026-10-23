import { ChevronDown } from 'lucide-react';
import { useStore } from '../store';

export interface Option<T extends string> {
  value: T;
  label: string;
}

/// A choice from a list, opened as the app's own menu (glass, rounded, a tick on the current
/// choice) rather than the web view's: Android's native picker is a grey system dialog that
/// matches nothing else. `placeholder` shows while nothing is chosen, for lists of actions such
/// as presets.
export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  placeholder = '',
  id,
}: {
  value: T | '';
  options: Option<T>[];
  onChange: (value: T) => void;
  label?: string;
  placeholder?: string;
  id?: string;
}) {
  const current = options.find((o) => o.value === value);
  return (
    <button
      type="button"
      id={id}
      className="select-button"
      aria-haspopup="menu"
      aria-label={label ? `${label}: ${current?.label ?? placeholder}` : undefined}
      onClick={(e) => {
        const box = e.currentTarget.getBoundingClientRect();
        useStore.getState().openMenu(
          box.left,
          box.bottom + 6,
          options.map((o) => ({
            label: o.label,
            checked: placeholder && !current ? undefined : o.value === value,
            run: () => onChange(o.value),
          })),
        );
      }}
    >
      <span>{current?.label ?? placeholder}</span>
      <ChevronDown size={14} aria-hidden />
    </button>
  );
}
