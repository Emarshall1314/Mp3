import { useEffect, useRef } from 'react';
import { api } from '../api';
import { useLook } from '../hooks/useAppearance';

const BANDS = 48;
/// Spectrum requests per second; bars ease between them at the display's frame rate.
const RATE = 30;

/// Bars for the playing audio's spectrum (40 Hz to 16 kHz), drawn in `color`. The engine only
/// copies audio for it while this is on screen.
export function Visualizer({ color }: { color: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const reduceMotion = useLook((s) => s.appearance.reduceMotion);
  const systemReduce =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const still = reduceMotion || systemReduce;

  useEffect(() => {
    if (still) return;
    void api.setVisualizer(true).catch(() => undefined);
    let target = new Array<number>(BANDS).fill(0);
    let shown = new Array<number>(BANDS).fill(0);
    let frame = 0;
    let live = true;
    let asking = false;
    const ask = setInterval(() => {
      if (asking) return;
      asking = true;
      api.spectrum(BANDS).then(
        (levels) => {
          asking = false;
          if (live) target = levels.length ? levels : target.map(() => 0);
        },
        () => {
          asking = false;
        },
      );
    }, 1000 / RATE);
    const draw = () => {
      const el = canvas.current;
      const ctx = el?.getContext('2d');
      if (el && ctx) {
        const dpr = window.devicePixelRatio || 1;
        const w = el.clientWidth * dpr;
        const h = el.clientHeight * dpr;
        if (el.width !== w || el.height !== h) {
          el.width = w;
          el.height = h;
        }
        // Rise quickly, fall slowly, like a meter.
        shown = shown.map((s, i) => {
          const t = target[i] ?? 0;
          return t > s ? s + (t - s) * 0.6 : s + (t - s) * 0.12;
        });
        ctx.clearRect(0, 0, w, h);
        ctx.fillStyle = color;
        const gap = 3 * dpr;
        const bar = (w - gap * (BANDS - 1)) / BANDS;
        shown.forEach((level, i) => {
          const bh = Math.max(2 * dpr, level * h);
          const x = i * (bar + gap);
          ctx.globalAlpha = 0.45 + 0.55 * level;
          ctx.beginPath();
          // Older WebKit has no roundRect: square bars there.
          const round = (ctx as Partial<CanvasRenderingContext2D>).roundRect;
          if (round) ctx.roundRect(x, h - bh, bar, bh, Math.min(bar / 2, 4 * dpr));
          else ctx.rect(x, h - bh, bar, bh);
          ctx.fill();
        });
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => {
      live = false;
      clearInterval(ask);
      cancelAnimationFrame(frame);
      void api.setVisualizer(false).catch(() => undefined);
    };
  }, [still, color]);

  if (still)
    return <p className="visualizer-still">The visualizer is off while Reduce Motion is on.</p>;
  return <canvas ref={canvas} className="visualizer" aria-hidden />;
}
