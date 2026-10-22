import { useRef, useState } from 'react';
import { CheckCircle2, Info } from 'lucide-react';
import { codecName, qualityLabel, signalPath } from '../format';
import { useStore } from '../store';
import type { Quality } from '../types';
import { QualityBadge } from './QualityBadge';
import { usePopover } from '../hooks/usePopover';

/// The quality badge in the player bar; clicking it shows each step from file to device and
/// whether the device gets the file's exact samples.
export function SignalPath({ quality, fallback }: { quality: Quality; fallback: string }) {
  const stream = useStore((s) => s.player?.stream ?? null);
  const volume = useStore((s) => s.player?.volume ?? 1);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  usePopover(open, setOpen, ref);

  const path = stream ? signalPath(stream, volume) : null;
  return (
    <div className="signal-path" ref={ref}>
      <button
        className="signal-button"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label="Signal path"
        onClick={() => setOpen(!open)}
        disabled={!path}
      >
        {qualityLabel(quality) || !stream ? (
          <QualityBadge quality={quality} detail={fallback} />
        ) : (
          <span className="quality-badge">{codecName(stream.codec)}</span>
        )}
        {path?.bitPerfect && <span className="bit-perfect">Bit-perfect</span>}
      </button>
      {open && path && (
        <div className="popover signal-popover left" role="dialog" aria-label="Signal path">
          <h2 className="popover-title">Signal Path</h2>
          <ol className="steps">
            {path.steps.map((s, i) => (
              <li key={i}>
                <span className="step-label">{s.label}</span>
                <span className="step-detail">{s.detail}</span>
              </li>
            ))}
          </ol>
          {path.bitPerfect ? (
            <p className="verdict good">
              <CheckCircle2 size={14} aria-hidden /> Bit-perfect: the device receives exactly the
              file’s samples.
            </p>
          ) : (
            <div className="verdict">
              <p>
                <Info size={14} aria-hidden /> Not bit-perfect:
              </p>
              <ul>
                {path.reasons.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
