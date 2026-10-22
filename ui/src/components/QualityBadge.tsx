import type { Quality } from '../types';
import { qualityLabel } from '../format';

/// "Lossless" / "Hi-Res Lossless" / "DSD", with the exact spec as a tooltip.
export function QualityBadge({ quality, detail }: { quality: Quality | null; detail?: string }) {
  const label = qualityLabel(quality);
  if (!label) return null;
  return (
    <span className={`quality-badge quality-${quality}`} title={detail}>
      {label}
    </span>
  );
}
