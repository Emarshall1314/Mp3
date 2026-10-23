import { useRef, useState } from 'react';

export const LETTERS = [...'ABCDEFGHIJKLMNOPQRSTUVWXYZ', '#'];

/// The index letters an artist is found under: the first letter of the name, and for names
/// starting "The …" also the first letter after it ("The Beatles": T and B). Anything not
/// starting with a letter is under #. Accents count as their letter (É is E).
export function artistLetters(name: string): string[] {
  const plain = name.normalize('NFD').replace(/[̀-ͯ]/g, '').trim().toUpperCase();
  const of = (s: string) => (/^[A-Z]/.test(s) ? s.charAt(0) : '#');
  const rest = /^THE\s+(.+)/.exec(plain)?.[1];
  return rest ? [...new Set(['T', of(rest)])] : [of(plain)];
}

/// How much a letter swells with the pointer `d` letters away: most under it, fading over the
/// next two or three, like the macOS dock.
export function swell(d: number): number {
  return 1 + 0.9 * Math.exp(-((d / 1.6) ** 2));
}

/// A vertical A–Z (and #) down the edge of a page, for jumping to the artists under a letter.
/// At rest it's a slim, quiet strip; pressed (or hovered with a mouse) it opens out and the
/// letters near the finger swell like the macOS dock, following the finger as it slides; the
/// letter under the finger when it lifts is chosen. Choosing the chosen letter again clears it.
/// Letters with nothing under them are dimmed and can't be chosen.
export function AlphabetIndex({
  available,
  value,
  onChange,
}: {
  available: Set<string>;
  value: string | null;
  onChange: (letter: string | null) => void;
}) {
  const strip = useRef<HTMLDivElement>(null);
  // Where the pointer is along the strip, in letters (fractional), while it's there.
  const [at, setAt] = useState<number | null>(null);
  const [held, setHeld] = useState(false);
  const last = useRef<string | null>(null);
  const position = (clientY: number) => {
    const box = strip.current?.getBoundingClientRect();
    if (!box) return null;
    const f = (clientY - box.top) / box.height;
    return Math.min(LETTERS.length - 0.5, Math.max(-0.5, f * LETTERS.length - 0.5));
  };
  const letterAt = (p: number) => LETTERS[Math.round(p)] ?? null;
  const track = (clientY: number) => {
    const p = position(clientY);
    setAt(p);
    const l = p === null ? null : letterAt(p);
    if (l && l !== last.current && available.has(l)) {
      last.current = l;
      // A tick under the finger as it passes each letter, where the device can.
      navigator.vibrate?.(4);
    }
  };
  const open = at !== null;
  return (
    <div
      className="alphabet"
      data-open={open || undefined}
      data-no-swipe=""
      role="toolbar"
      aria-label="Jump to letter"
      onPointerEnter={(e) => {
        if (e.pointerType === 'mouse') track(e.clientY);
      }}
      onPointerLeave={(e) => {
        if (e.pointerType === 'mouse' && !held) setAt(null);
      }}
      onPointerDown={(e) => {
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        setHeld(true);
        last.current = null;
        track(e.clientY);
      }}
      onPointerMove={(e) => {
        if (held || e.pointerType === 'mouse') track(e.clientY);
      }}
      onPointerUp={(e) => {
        const p = position(e.clientY);
        setHeld(false);
        if (e.pointerType !== 'mouse') setAt(null);
        const l = p === null ? null : letterAt(p);
        if (l && available.has(l)) onChange(l === value ? null : l);
      }}
      onPointerCancel={() => {
        setHeld(false);
        setAt(null);
      }}
      onKeyDown={(e) => {
        const step = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : 0;
        if (!step) return;
        e.preventDefault();
        const buttons = [...(strip.current?.querySelectorAll('button') ?? [])];
        const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[Math.min(buttons.length - 1, Math.max(0, i + step))]?.focus();
      }}
    >
      <div ref={strip} className="alphabet-strip">
        {LETTERS.map((l, i) => {
          const s = at === null ? 1 : swell(Math.abs(i - at));
          return (
            <button
              key={l}
              // One tab stop; arrow keys move along the letters.
              tabIndex={i === 0 ? 0 : -1}
              aria-pressed={value === l}
              aria-disabled={!available.has(l) || undefined}
              data-on={value === l || undefined}
              style={{ scale: String(s), translate: `${-(s - 1) * 18}px 0` }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && available.has(l)) onChange(l === value ? null : l);
              }}
            >
              {l}
            </button>
          );
        })}
      </div>
    </div>
  );
}
