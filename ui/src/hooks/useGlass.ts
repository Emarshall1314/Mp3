// Liquid glass on the phone (D58): lenses sized to each glass shape, light on the rims that
// follows how the phone is tilted, and a glow where a finger presses the glass.

import { type RefObject, useEffect } from 'react';
import { useLook } from './useAppearance';
import { lensFilter } from '../theme/liquid';

const motionQuery = '(prefers-reduced-motion: reduce)';

/// Whether panels are liquid glass right now (the look asks for it and nothing turns it off).
export function useLiquid(): boolean {
  return useLook(
    (s) =>
      s.look.panels === 'liquid' &&
      !s.appearance.reduceTransparency &&
      !matchMedia('(prefers-reduced-transparency: reduce)').matches,
  );
}

function reducedMotion(): boolean {
  return (
    document.documentElement.hasAttribute('data-reduce-motion') || matchMedia(motionQuery).matches
  );
}

/// Gives the element a lens filter fitted to its size and corner radius, as `--lens`, which the
/// liquid glass styles put in its `backdrop-filter` (Chromium only; nothing happens elsewhere).
export function useLens(ref: RefObject<HTMLElement | null>, on: boolean) {
  useEffect(() => {
    const el = ref.current;
    if (!el || !on) return;
    const fit = () => {
      const { width, height } = el.getBoundingClientRect();
      const radius = parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0;
      const id = lensFilter(width, height, Math.min(radius, width / 2, height / 2));
      if (id) el.style.setProperty('--lens', `url(#${id})`);
      else el.style.removeProperty('--lens');
    };
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    return () => {
      observer.disconnect();
      el.style.removeProperty('--lens');
    };
  }, [ref, on]);
}

/// Moves the light on glass rims as the phone tilts (`--glass-light`, an angle). Still when
/// Reduce Motion is on or the device has no motion sensor.
export function useGlassLight(on: boolean) {
  useEffect(() => {
    if (!on || reducedMotion()) return;
    const root = document.documentElement;
    let frame = 0;
    let angle = 135;
    const onTilt = (e: DeviceOrientationEvent) => {
      if (e.gamma === null || e.beta === null) return;
      // Left-right tilt swings the light round; holding the phone upright is the rest angle.
      const swing = Math.max(-60, Math.min(60, e.gamma * 1.5 + (e.beta - 50) * 0.4));
      angle = 135 + swing;
      if (!frame)
        frame = requestAnimationFrame(() => {
          frame = 0;
          root.style.setProperty('--glass-light', `${angle.toFixed(1)}deg`);
        });
    };
    window.addEventListener('deviceorientation', onTilt);
    return () => {
      window.removeEventListener('deviceorientation', onTilt);
      cancelAnimationFrame(frame);
      root.style.removeProperty('--glass-light');
    };
  }, [on]);
}

/// Pressing glass (anything `.glass-lit`) lights it from the point of contact and lets it swell
/// a little, as Apple's does: `data-pressed` and `--touch-x` / `--touch-y` on the element.
export function useGlassTouch(on: boolean) {
  useEffect(() => {
    if (!on) return;
    let pressed: HTMLElement | null = null;
    const place = (el: HTMLElement, e: PointerEvent) => {
      const box = el.getBoundingClientRect();
      el.style.setProperty('--touch-x', `${e.clientX - box.left}px`);
      el.style.setProperty('--touch-y', `${e.clientY - box.top}px`);
    };
    const down = (e: PointerEvent) => {
      const el = (e.target as Element | null)?.closest<HTMLElement>('.glass-lit');
      if (!el) return;
      pressed = el;
      place(el, e);
      el.setAttribute('data-pressed', '');
    };
    const move = (e: PointerEvent) => {
      if (pressed) place(pressed, e);
    };
    const up = () => {
      pressed?.removeAttribute('data-pressed');
      pressed = null;
    };
    document.addEventListener('pointerdown', down, true);
    document.addEventListener('pointermove', move, true);
    document.addEventListener('pointerup', up, true);
    document.addEventListener('pointercancel', up, true);
    return () => {
      up();
      document.removeEventListener('pointerdown', down, true);
      document.removeEventListener('pointermove', move, true);
      document.removeEventListener('pointerup', up, true);
      document.removeEventListener('pointercancel', up, true);
    };
  }, [on]);
}
