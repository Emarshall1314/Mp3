import { Minus, Plus } from 'lucide-react';

/// Smaller or bigger cards in a grid: a small capsule of − and +, with dots for where it is.
export function GridZoom({
  value,
  levels,
  onChange,
}: {
  value: number;
  levels: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="grid-zoom" role="group" aria-label="Card size">
      <button
        className="icon-button small"
        aria-label="Smaller"
        disabled={value <= 0}
        onClick={() => onChange(value - 1)}
      >
        <Minus size={16} />
      </button>
      <span className="grid-zoom-dots" aria-hidden>
        {Array.from({ length: levels }, (_, i) => (
          <span key={i} data-on={i === value || undefined} />
        ))}
      </span>
      <button
        className="icon-button small"
        aria-label="Bigger"
        disabled={value >= levels - 1}
        onClick={() => onChange(value + 1)}
      >
        <Plus size={16} />
      </button>
    </div>
  );
}
