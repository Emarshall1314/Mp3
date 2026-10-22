import type { InputHTMLAttributes } from 'react';

/// A range input whose filled track follows its value.
export function Range({
  value,
  min,
  max,
  style,
  ...rest
}: Omit<InputHTMLAttributes<HTMLInputElement>, 'type' | 'value' | 'min' | 'max'> & {
  value: number;
  min: number;
  max: number;
}) {
  const progress = max > min ? ((value - min) / (max - min)) * 100 : 0;
  return (
    <input
      type="range"
      value={value}
      min={min}
      max={max}
      style={{ ...style, ['--progress' as string]: `${progress}%` }}
      {...rest}
    />
  );
}
