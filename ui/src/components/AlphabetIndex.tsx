import { useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useLens, useLiquid } from '../hooks/useGlass';

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
  live = false,
}: {
  available: Set<string>;
  value: string | null;
  onChange: (letter: string | null) => void;
  /// Jump as the finger passes each letter (long sorted lists) rather than when it lifts
  /// (narrowing a grid).
  live?: boolean;
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
      setShown(l);
      if (live) onChange(l);
      // A tick under the finger as it passes each letter, where the device can.
      navigator.vibrate?.(4);
    }
  };
  // The letter in the island at the top of the screen, and whether it's on its way out.
  const [shown, setShown] = useState<string | null>(null);
  const [leaving, setLeaving] = useState(false);
  const leave = () => {
    setLeaving(true);
    setTimeout(() => {
      setShown(null);
      setLeaving(false);
    }, 240);
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
        if (e.pointerType === 'mouse' && !held) {
          setAt(null);
          leave();
        }
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
        if (e.pointerType !== 'mouse') {
          setAt(null);
          leave();
        }
        const l = p === null ? null : letterAt(p);
        if (l && available.has(l) && !live) onChange(l === value ? null : l);
      }}
      onPointerCancel={() => {
        setHeld(false);
        setAt(null);
        leave();
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
      {shown &&
        createPortal(
          <LetterIsland letter={shown} leaving={leaving} />,
          document.querySelector('.app') ?? document.body,
        )}
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

/// The letter being scrubbed to, large, in a round liquid glass lens at the top of the screen,
/// out from under the finger: it grows out of a small capsule like the iPhone's Dynamic Island,
/// pops as the letter changes, and shrinks away when the finger lifts.
function LetterIsland({ letter, leaving }: { letter: string; leaving: boolean }) {
  const liquid = useLiquid();
  const lens = useRef<HTMLDivElement>(null);
  useLens(lens, liquid);
  return (
    <div
      ref={lens}
      className={`letter-island${liquid ? ' glass-lit' : ''}`}
      data-leaving={leaving || undefined}
      aria-live="polite"
    >
      <span key={letter}>{letter}</span>
    </div>
  );
}
