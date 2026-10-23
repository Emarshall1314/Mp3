import { useState } from 'react';
import { Disc3 } from 'lucide-react';
import { artUrl } from '../api';

interface Props {
  hash: string | null;
  size: number;
  alt: string;
  className?: string;
}

/// Album art from the thumbnail cache, or a quiet placeholder when there is none (or it fails).
/// When the picture changes, the old one stays until the new one has loaded, which then fades in
/// over it: no flash of the placeholder between songs. A failed load is tried once more (the
/// core may still have been opening) before the placeholder shows.
export function Artwork({ hash, size, alt, className }: Props) {
  const [failed, setFailed] = useState<{ hash: string; tries: number } | null>(null);
  // The last picture that finished loading, shown under the new one until it is ready.
  const [shown, setShown] = useState<string | null>(null);
  const tries = failed?.hash === hash ? failed.tries : 0;
  const broken = tries >= 2;
  const src = hash && !broken ? artUrl(hash, size) + (tries ? '&retry=1' : '') : null;
  const previous = shown && shown !== src ? shown : null;
  return (
    <div className={`artwork ${className ?? ''}`} style={{ width: size, height: size }}>
      {previous && <img className="artwork-previous" src={previous} alt="" aria-hidden />}
      {src ? (
        <img
          key={src}
          src={src}
          alt={alt}
          loading="lazy"
          decoding="async"
          data-fade={previous ? '' : undefined}
          onLoad={() => setShown(src)}
          onError={() => {
            if (!hash) return;
            if (tries === 0) setTimeout(() => setFailed({ hash, tries: 1 }), 800);
            else setFailed({ hash, tries: 2 });
          }}
        />
      ) : (
        <div className="artwork-placeholder" role="img" aria-label={alt}>
          <Disc3 size={Math.max(16, size * 0.35)} strokeWidth={1.25} />
        </div>
      )}
    </div>
  );
}
