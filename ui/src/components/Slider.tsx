import { useRef, useState } from 'react';

/// A horizontal slider driven by pointer events (mouse, touch and pen alike), for seeking and
/// volume. The web view's own range input lost the release on Android touch and snapped back
/// before a seek landed; this one follows the finger (with pointer capture, so it keeps
/// following off the bar), reports the value while dragging (`onInput`) and once let go
/// (`onCommit`), and thickens while held. Arrow keys move by `step`, Page keys by ten steps.
export function Slider({
  value,
  label,
  valueText,
  step,
  disabled = false,
  onInput,
  onCommit,
}: {
  /// 0 to 1.
  value: number;
  label: string;
  valueText?: string;
  /// For the keyboard, as a fraction.
  step: number;
  disabled?: boolean;
  onInput?: (v: number) => void;
  onCommit: (v: number) => void;
}) {
  const bar = useRef<HTMLDivElement>(null);
  const [held, setHeld] = useState(false);
  const at = (clientX: number) => {
    const box = bar.current?.getBoundingClientRect();
    if (!box || box.width === 0) return value;
    return Math.min(1, Math.max(0, (clientX - box.left) / box.width));
  };
  const v = Math.min(1, Math.max(0, value));
  return (
    <div
      ref={bar}
      className="slider"
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(v * 100)}
      aria-valuetext={valueText}
      aria-disabled={disabled || undefined}
      data-held={held || undefined}
      data-no-swipe=""
      style={{ ['--value' as string]: `${v * 100}%` }}
      onPointerDown={(e) => {
        if (disabled || e.button !== 0) return;
        e.preventDefault();
        e.stopPropagation();
        e.currentTarget.setPointerCapture(e.pointerId);
        setHeld(true);
        onInput?.(at(e.clientX));
      }}
      onPointerMove={(e) => {
        if (held) onInput?.(at(e.clientX));
      }}
      onPointerUp={(e) => {
        if (!held) return;
        setHeld(false);
        onCommit(at(e.clientX));
      }}
      onPointerCancel={() => {
        // The system took the touch: keep where the finger last was.
        if (!held) return;
        setHeld(false);
        onCommit(v);
      }}
      onKeyDown={(e) => {
        if (disabled) return;
        const by: Record<string, number> = {
          ArrowRight: step,
          ArrowUp: step,
          ArrowLeft: -step,
          ArrowDown: -step,
          PageUp: step * 10,
          PageDown: -step * 10,
        };
        const d = by[e.key];
        const to = e.key === 'Home' ? 0 : e.key === 'End' ? 1 : d !== undefined ? v + d : null;
        if (to === null) return;
        e.preventDefault();
        onCommit(Math.min(1, Math.max(0, to)));
      }}
    >
      <span className="slider-track">
        <span className="slider-fill" />
      </span>
      <span className="slider-thumb" />
    </div>
  );
}
