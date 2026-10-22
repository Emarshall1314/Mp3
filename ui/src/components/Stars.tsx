import { useState } from 'react';
import { Star } from 'lucide-react';
import { api } from '../api';
import { attempt } from '../store';

/// Five stars for a song's rating (0–100 in steps of 20). Clicking the current rating clears it.
/// Unrated songs show the stars only on hover or focus.
export function Stars({ trackId, rating }: { trackId: number; rating: number | null }) {
  const [hover, setHover] = useState<number | null>(null);
  const stars = Math.round((rating ?? 0) / 20);
  const shown = hover ?? stars;
  return (
    <span
      className={`stars ${stars ? 'rated' : ''}`}
      role="radiogroup"
      aria-label="Rating"
      onMouseLeave={() => setHover(null)}
    >
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          role="radio"
          aria-checked={n === stars}
          aria-label={n === 1 ? '1 star' : `${n} stars`}
          title={n === stars ? 'Clear the rating' : undefined}
          className={n <= shown ? 'on' : ''}
          onMouseEnter={() => setHover(n)}
          onClick={(e) => {
            e.stopPropagation();
            void attempt(api.rate([trackId], n === stars ? null : n * 20));
          }}
        >
          <Star size={12} fill={n <= shown ? 'currentColor' : 'none'} aria-hidden />
        </button>
      ))}
    </span>
  );
}
