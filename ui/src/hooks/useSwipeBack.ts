// Swiping right on a phone page goes back, as on an iPhone: the page follows the finger and the
// page before (kept alive underneath by the page stack) slides in behind it from a little to
// the left, dimmed, brightening as it comes. Let go far enough (or flick) and the move
// finishes; otherwise it springs back. On a tab's top page, a swipe right brings Now Playing
// in from the left instead (when something is playing), so the loop Now Playing → album →
// albums → Now Playing goes round.

import { type RefObject, useEffect } from 'react';
import { useStore } from '../store';

/// Where a horizontal swipe must not start: things that scroll or slide sideways themselves.
export const NO_SWIPE = '.card-row, input, textarea, .scrubber, .slider, [data-no-swipe]';

/// Whether a drag has shown which way it's going: `null` until it has moved far enough.
export function swipeAxis(dx: number, dy: number): 'x' | 'y' | null {
  if (Math.hypot(dx, dy) < 12) return null;
  return Math.abs(dx) > Math.abs(dy) * 1.5 ? 'x' : 'y';
}

/// Whether a release finishes the swipe: far enough across, or flicked.
export function swipeDone(distance: number, ms: number, width: number): boolean {
  return distance > width * 0.3 || (distance > 30 && distance / Math.max(ms, 1) > 0.6);
}

function reducedMotion() {
  return document.documentElement.hasAttribute('data-reduce-motion');
}

const SPRING = '420ms var(--ease-spring)';
/// How far left the page underneath starts, as a fraction of the width.
const PARALLAX = 0.3;

function clear(el: HTMLElement | null) {
  if (!el) return;
  el.style.transition = '';
  el.style.transform = '';
  el.style.opacity = '';
  el.style.visibility = '';
  el.style.animation = '';
  el.removeAttribute('data-moving');
}

export function useSwipeBack(
  ref: RefObject<HTMLElement | null>,
  on: boolean,
  atTabTop: () => boolean,
) {
  useEffect(() => {
    const main = ref.current;
    if (!main || !on) return;
    let start: { x: number; y: number; t: number } | null = null;
    let axis: 'x' | 'y' | null = null;
    let mode: 'back' | 'nowPlaying' | 'stuck' | null = null;
    let swiped = 0;
    const top = () => main.querySelector<HTMLElement>(':scope > .page-layer:not([data-under])');
    const under = () => main.querySelector<HTMLElement>(':scope > .page-layer[data-under]');
    const sheet = () => document.querySelector<HTMLElement>('.now-playing-view');
    const width = () => main.clientWidth || innerWidth;

    const down = (e: PointerEvent) => {
      if (e.button !== 0 || (e.target as Element).closest(NO_SWIPE)) return;
      start = { x: e.clientX, y: e.clientY, t: e.timeStamp };
      axis = null;
      mode = null;
    };
    const move = (e: PointerEvent) => {
      if (!start) return;
      const dx = e.clientX - start.x;
      axis ??= swipeAxis(dx, e.clientY - start.y);
      if (axis === 'y') {
        start = null;
        return;
      }
      if (axis !== 'x') return;
      if (!mode) {
        const s = useStore.getState();
        mode =
          s.player?.trackId != null && (atTabTop() || s.back.length === 0)
            ? 'nowPlaying'
            : s.back.length > 0
              ? 'back'
              : 'stuck';
        if (mode === 'nowPlaying') {
          // Now Playing comes in from the left, following the finger.
          s.setNowPlaying(true, true);
        }
      }
      const d = Math.max(0, dx);
      const w = width();
      if (mode === 'nowPlaying') {
        const el = sheet();
        if (!el) return;
        el.style.animation = 'none';
        el.style.transition = 'none';
        el.style.transform = `translateX(${d - w}px)`;
        return;
      }
      const page = top();
      if (!page) return;
      page.setAttribute('data-moving', '');
      page.style.transition = 'none';
      if (mode === 'stuck') {
        // Nowhere to go back to: a quarter of the movement, as a hint.
        page.style.transform = `translateX(${d * 0.25}px)`;
        return;
      }
      page.style.transform = `translateX(${d}px)`;
      const below = under();
      if (below) {
        const p = Math.min(1, d / w);
        below.style.visibility = 'visible';
        below.style.transition = 'none';
        below.style.transform = `translateX(${-(1 - p) * PARALLAX * w}px)`;
        below.style.opacity = String(0.55 + 0.45 * p);
      }
    };
    const up = (e: PointerEvent) => {
      const from = start;
      start = null;
      if (!from || axis !== 'x' || !mode) return;
      swiped = e.timeStamp;
      const dx = Math.max(0, e.clientX - from.x);
      const w = width();
      const done = swipeDone(dx, e.timeStamp - from.t, w);
      const quick = reducedMotion() ? '0ms' : '260ms';
      if (mode === 'nowPlaying') {
        const el = sheet();
        if (!el) return;
        if (done) {
          el.style.transition = `transform ${SPRING}`;
          el.style.transform = '';
          setTimeout(() => clear(el), 450);
        } else {
          el.style.transition = `transform ${quick} ease-in`;
          el.style.transform = `translateX(${-w}px)`;
          setTimeout(() => useStore.getState().setNowPlaying(false), 250);
        }
        mode = null;
        return;
      }
      const page = top();
      const below = under();
      if (mode === 'back' && done && page) {
        page.style.transition = `transform ${quick} cubic-bezier(0.2, 0.7, 0.3, 1)`;
        page.style.transform = `translateX(${w}px)`;
        if (below) {
          below.style.transition = `transform ${quick} cubic-bezier(0.2, 0.7, 0.3, 1), opacity ${quick}`;
          below.style.transform = '';
          below.style.opacity = '';
        }
        setTimeout(
          () => {
            useStore.getState().goBack();
            // The page underneath is now the current one (the same element): make it plain.
            requestAnimationFrame(() => clear(top()));
          },
          reducedMotion() ? 0 : 250,
        );
      } else {
        if (page) {
          page.style.transition = `transform ${SPRING}`;
          page.style.transform = '';
          setTimeout(() => clear(page), 450);
        }
        if (below) {
          below.style.transition = `transform ${SPRING}, opacity 300ms`;
          below.style.transform = `translateX(${-PARALLAX * w}px)`;
          below.style.opacity = '0.55';
          setTimeout(() => clear(below), 450);
        }
      }
      mode = null;
    };
    // A swipe isn't also a tap on whatever it started on.
    const click = (e: MouseEvent) => {
      if (e.timeStamp - swiped < 400) {
        e.stopPropagation();
        e.preventDefault();
      }
    };
    main.addEventListener('pointerdown', down);
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', up);
    main.addEventListener('click', click, true);
    return () => {
      main.removeEventListener('pointerdown', down);
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', up);
      main.removeEventListener('click', click, true);
    };
  }, [ref, on, atTabTop]);
}
