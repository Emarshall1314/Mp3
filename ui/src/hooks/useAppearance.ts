import { useEffect } from 'react';
import { api, onAppearance } from '../api';
import type { Appearance } from '../types';

const motionQuery = '(prefers-reduced-motion: reduce)';
const transparencyQuery = '(prefers-reduced-transparency: reduce)';

let current: Appearance = { theme: 'auto', reduceMotion: false, reduceTransparency: false };

/// Puts the appearance on <html>: `data-appearance` (light/dark; absent follows the desktop),
/// and `data-reduce-motion` / `data-reduce-transparency` when the app setting or the desktop's
/// own preference asks for it. The stylesheet keys off these attributes only.
export function applyAppearance(a: Appearance) {
  current = a;
  const root = document.documentElement;
  if (a.theme === 'auto') delete root.dataset.appearance;
  else root.dataset.appearance = a.theme;
  const set = (name: string, on: boolean) => {
    if (on) root.setAttribute(name, '');
    else root.removeAttribute(name);
  };
  set('data-reduce-motion', a.reduceMotion || matchMedia(motionQuery).matches);
  set('data-reduce-transparency', a.reduceTransparency || matchMedia(transparencyQuery).matches);
}

/// Loads the saved appearance and follows changes made in any window or on the desktop.
export function useAppearance() {
  useEffect(() => {
    void api.settings().then((s) => applyAppearance(s.appearance));
    const unlisten = onAppearance(applyAppearance);
    const media = [matchMedia(motionQuery), matchMedia(transparencyQuery)];
    const onMedia = () => applyAppearance(current);
    media.forEach((m) => m.addEventListener('change', onMedia));
    return () => {
      void unlisten.then((f) => f());
      media.forEach((m) => m.removeEventListener('change', onMedia));
    };
  }, []);
}
