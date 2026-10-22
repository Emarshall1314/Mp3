import { LayoutGrid, List } from 'lucide-react';
import type { GridOrList } from '../theme/layout';

/// Grid or list for a section; the choice becomes the section's default view.
export function ViewToggle({
  value,
  onChange,
}: {
  value: GridOrList;
  onChange: (v: GridOrList) => void;
}) {
  return (
    <div className="segmented icons" role="radiogroup" aria-label="View">
      {(
        [
          ['grid', 'Grid', LayoutGrid],
          ['list', 'List', List],
        ] as const
      ).map(([v, label, Icon]) => (
        <button
          key={v}
          role="radio"
          aria-checked={value === v}
          aria-label={label}
          title={label}
          className={value === v ? 'on' : ''}
          onClick={() => onChange(v)}
        >
          <Icon size={14} aria-hidden />
        </button>
      ))}
    </div>
  );
}
