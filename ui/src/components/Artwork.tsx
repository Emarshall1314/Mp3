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
export function Artwork({ hash, size, alt, className }: Props) {
  const [failed, setFailed] = useState<string | null>(null);
  const show = hash && failed !== hash;
  return (
    <div className={`artwork ${className ?? ''}`} style={{ width: size, height: size }}>
      {show ? (
        <img
          src={artUrl(hash, size)}
          alt={alt}
          loading="lazy"
          decoding="async"
          onError={() => setFailed(hash)}
        />
      ) : (
        <div className="artwork-placeholder" role="img" aria-label={alt}>
          <Disc3 size={Math.max(16, size * 0.35)} strokeWidth={1.25} />
        </div>
      )}
    </div>
  );
}
