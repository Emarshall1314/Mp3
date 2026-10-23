// Swiping right on a phone page goes back (as on an iPhone): the page follows the finger, and
// let go far enough (or flicked) it slides away and the page before eases in; otherwise it
// springs back. With nowhere to go back to, it only gives a little.

import { type RefObject, useEffect } from 'react';
import { useStore } from '../store';

/// Where a horizontal swipe must not start: things that scroll or slide sideways themselves.
export const NO_SWIPE = '.card-row, input, textarea, .scrubber, [data-no-swipe]';

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

export function useSwipeBack(ref: RefObject<HTMLElement | null>, on: boolean) {
  useEffect(() => {
    const main = ref.current;
    if (!main || !on) return;
    let start: { x: number; y: number; t: number } | null = null;
    let axis: 'x' | 'y' | null = null;
    let page: HTMLElement | null = null;
    let swiped = 0;
    const view = () => main.querySelector<HTMLElement>(':scope > .view') ?? null;
    const canBack = () => useStore.getState().back.length > 0;

    const down = (e: PointerEvent) => {
      if (e.button !== 0 || (e.target as Element).closest(NO_SWIPE)) return;
      start = { x: e.clientX, y: e.clientY, t: e.timeStamp };
      axis = null;
    };
    const move = (e: PointerEvent) => {
      if (!start) return;
      const dx = e.clientX - start.x;
      axis ??= swipeAxis(dx, e.clientY - start.y);
      if (axis !== 'x' || dx <= 0) {
        if (axis === 'y') start = null;
        return;
      }
      page ??= view();
      if (!page) return;
      // Nowhere to go back to: a quarter of the movement, as a hint.
      const shift = canBack() ? dx : dx * 0.25;
      page.style.transition = 'none';
      page.style.transform = `translateX(${shift}px)`;
      page.style.opacity = String(Math.max(0.4, 1 - shift / (innerWidth * 1.5)));
    };
    const up = (e: PointerEvent) => {
      const from = start;
      start = null;
      if (!from || axis !== 'x' || !page) {
        page = null;
        return;
      }
      swiped = e.timeStamp;
      const dx = e.clientX - from.x;
      const leaving = page;
      page = null;
      const spring = 'transform 420ms var(--ease-spring), opacity 300ms';
      if (canBack() && swipeDone(dx, e.timeStamp - from.t, innerWidth)) {
        leaving.style.transition = 'transform 200ms ease-in, opacity 200ms';
        leaving.style.transform = `translateX(${innerWidth}px)`;
        leaving.style.opacity = '0';
        setTimeout(() => {
          useStore.getState().goBack();
          // The page before eases in from the left (React may reuse the same element).
          requestAnimationFrame(() => {
            const next = view();
            if (!next) return;
            next.style.transition = 'none';
            next.style.transform = reducedMotion() ? '' : 'translateX(-40px)';
            next.style.opacity = reducedMotion() ? '' : '0';
            void next.offsetWidth;
            next.style.transition = spring;
            next.style.transform = '';
            next.style.opacity = '';
          });
        }, 190);
      } else {
        leaving.style.transition = spring;
        leaving.style.transform = '';
        leaving.style.opacity = '';
      }
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
  }, [ref, on]);
}
