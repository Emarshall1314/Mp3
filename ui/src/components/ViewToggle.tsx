import { Columns3, LayoutGrid, List } from 'lucide-react';
import type { AlbumsView } from '../theme/layout';

/// Grid, list (or columns) for a section; the choice becomes the section's default view.
export function ViewToggle<T extends AlbumsView>({
  value,
  onChange,
  columns = false,
}: {
  value: T;
  onChange: (v: T) => void;
  /// Offer the column browser too.
  columns?: boolean;
}) {
  return (
    <div className="segmented icons" role="radiogroup" aria-label="View">
      {(
        [
          ['grid', 'Grid', LayoutGrid],
          ['list', 'List', List],
          ...(columns ? ([['columns', 'Column browser', Columns3]] as const) : []),
        ] as const
      ).map(([v, label, Icon]) => (
        <button
          key={v}
          role="radio"
          aria-checked={value === v}
          aria-label={label}
          title={label}
          className={value === v ? 'on' : ''}
          onClick={() => onChange(v as T)}
        >
          <Icon size={14} aria-hidden />
        </button>
      ))}
    </div>
  );
}
