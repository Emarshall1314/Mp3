// The phone dock's glass reacts to scrolling, a little: each floating piece (the tabs, the player,
// the Search bubble) is a damped spring tethered to its place, and the page sliding under it
// drags it along in proportion to how fast it's going. Scroll briskly and the pieces lean with
// the page; stop and they spring back, overshooting slightly. Each has its own stiffness, so they
// move a touch out of step, as separate objects would.

import { type RefObject, useEffect } from 'react';

export interface Body {
  /// Displacement from rest, px (negative: up).
  x: number;
  /// Velocity, px/ms.
  v: number;
}

export interface Spring {
  /// Natural frequency, Hz.
  hz: number;
  /// Damping ratio (below 1 overshoots).
  zeta: number;
  /// How far the page's speed (px/ms) pulls it, px per px/ms.
  drag: number;
}

/// The furthest a piece leans, px.
export const LEAN = 6;

/// Advances a body by `dt` ms with the page moving at `page` px/ms (semi-implicit Euler, which
/// stays stable at frame-sized steps).
export function step(b: Body, s: Spring, page: number, dt: number): Body {
  const w = 2 * Math.PI * s.hz * 0.001; // rad/ms
  const target = Math.max(-LEAN, Math.min(LEAN, s.drag * page));
  const a = -w * w * (b.x - target) - 2 * s.zeta * w * b.v;
  const v = b.v + a * dt;
  return { x: b.x + v * dt, v };
}

const PIECES: { selector: string; spring: Spring }[] = [
  { selector: '.phone-tabs', spring: { hz: 2.6, zeta: 0.32, drag: 1.6 } },
  { selector: '.phone-player', spring: { hz: 3.0, zeta: 0.3, drag: 1.9 } },
  { selector: '.phone-search-button', spring: { hz: 3.6, zeta: 0.34, drag: 1.4 } },
];

export function useDockPhysics(dock: RefObject<HTMLElement | null>, on: boolean) {
  useEffect(() => {
    const root = dock.current;
    if (!root || !on || document.documentElement.hasAttribute('data-reduce-motion')) return;
    const bodies: Body[] = PIECES.map(() => ({ x: 0, v: 0 }));
    // The page's speed (px/ms; negative while its content moves up), and when it last moved.
    let page = 0;
    let lastScroll = 0;
    const tops = new WeakMap<EventTarget, { top: number; t: number }>();
    let frame = 0;
    let then = 0;

    const tick = (now: number) => {
      const dt = Math.min(32, now - then || 16);
      then = now;
      // With no scrolling for a moment, the page has stopped.
      if (now - lastScroll > 60) page *= Math.exp(-dt / 40);
      let moving = Math.abs(page) > 0.001;
      PIECES.forEach((p, i) => {
        const b = step(bodies[i] ?? { x: 0, v: 0 }, p.spring, page, dt);
        bodies[i] = b;
        if (Math.abs(b.x) > 0.02 || Math.abs(b.v) > 0.0005) moving = true;
        const el = root.querySelector<HTMLElement>(p.selector);
        if (!el) return;
        el.style.setProperty('--wy', `${b.x.toFixed(2)}px`);
        // Stretched a hair along the way it's moving.
        el.style.setProperty('--ws', String(1 + Math.max(-0.03, Math.min(0.03, b.v * 0.05))));
      });
      if (moving) frame = requestAnimationFrame(tick);
      else {
        frame = 0;
        PIECES.forEach((p) => {
          const el = root.querySelector<HTMLElement>(p.selector);
          el?.style.removeProperty('--wy');
          el?.style.removeProperty('--ws');
        });
      }
    };

    const onScroll = (e: Event) => {
      const el = e.target;
      if (!(el instanceof HTMLElement) || !el.closest('.phone-main')) return;
      const was = tops.get(el);
      tops.set(el, { top: el.scrollTop, t: e.timeStamp });
      if (!was || was.top === el.scrollTop) return;
      const dt = Math.max(1, e.timeStamp - was.t);
      const speed = -(el.scrollTop - was.top) / dt;
      page = page * 0.6 + Math.max(-4, Math.min(4, speed)) * 0.4;
      lastScroll = performance.now();
      if (!frame) {
        then = performance.now();
        frame = requestAnimationFrame(tick);
      }
    };
    document.addEventListener('scroll', onScroll, { capture: true, passive: true });
    return () => {
      document.removeEventListener('scroll', onScroll, { capture: true });
      cancelAnimationFrame(frame);
    };
  }, [dock, on]);
}
