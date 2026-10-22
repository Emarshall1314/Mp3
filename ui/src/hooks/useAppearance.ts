import { useEffect } from 'react';
import { create } from 'zustand';
import { api, onAppearance, onThemesChanged } from '../api';
import {
  type Look,
  type Rgb,
  type Scheme,
  type ThemeDef,
  buildLook,
  normaliseLook,
  themeFor,
} from '../theme/look';
import { ensureLiquidFilter, refracts } from '../theme/liquid';
import type { Layout } from '../theme/layout';
import { attempt } from '../store';
import type { Appearance, SavedTheme } from '../types';

const motionQuery = '(prefers-reduced-motion: reduce)';
const transparencyQuery = '(prefers-reduced-transparency: reduce)';
const darkQuery = '(prefers-color-scheme: dark)';

interface LookState {
  appearance: Appearance;
  look: Look;
  /// The owner's themes (built-in ones are in theme/look).
  saved: ThemeDef[];
  /// The current artwork's colours, when the look uses them.
  palette: Rgb[];
  /// A theme being edited, shown in place of the chosen one.
  preview: ThemeDef | null;
  /// What is on screen now.
  scheme: Scheme;
  theme: ThemeDef;
}

export const useLook = create<LookState>(() => ({
  appearance: { theme: 'auto', reduceMotion: false, reduceTransparency: false, look: null },
  look: normaliseLook(null),
  saved: [],
  palette: [],
  preview: null,
  scheme: 'light',
  theme: themeFor(normaliseLook(null), 'light', []),
}));

export const toThemeDef = (s: SavedTheme): ThemeDef => ({
  id: s.id,
  name: s.theme.name,
  scheme: s.theme.scheme,
  tokens: s.theme.tokens,
});

let applied: string[] = [];

/// Puts the look on <html>: the theme's tokens with the look settings applied as inline custom
/// properties, `data-*` attributes for styles that need selectors, and `data-reduce-motion` /
/// `data-reduce-transparency` when the app setting or the desktop's own preference asks.
function apply() {
  const s = useLook.getState();
  const a = s.appearance;
  const scheme: Scheme =
    a.theme === 'auto' ? (matchMedia(darkQuery).matches ? 'dark' : 'light') : a.theme;
  const theme = s.preview ?? themeFor(s.look, scheme, s.saved);
  const reduceTransparency = a.reduceTransparency || matchMedia(transparencyQuery).matches;
  const { vars, attrs } = buildLook(s.look, theme, s.palette, reduceTransparency);
  const root = document.documentElement;
  for (const name of applied) if (!(name in vars)) root.style.removeProperty(name);
  for (const [name, value] of Object.entries(vars)) root.style.setProperty(name, value);
  applied = Object.keys(vars);
  root.style.colorScheme = theme.scheme;
  root.style.fontSize = `${s.look.fontSize}%`;
  for (const [name, value] of Object.entries(attrs)) root.setAttribute(name, value);
  const flag = (name: string, on: boolean) => {
    if (on) root.setAttribute(name, '');
    else root.removeAttribute(name);
  };
  flag('data-reduce-motion', a.reduceMotion || matchMedia(motionQuery).matches);
  flag('data-reduce-transparency', reduceTransparency);
  const refraction = attrs['data-panels'] === 'liquid' && refracts;
  if (refraction) ensureLiquidFilter();
  flag('data-refraction', refraction);
  if (s.scheme !== theme.scheme || s.theme !== theme)
    useLook.setState({ scheme: theme.scheme, theme });
}

/// Shows an appearance at once (before or without saving it).
export function applyAppearance(a: Appearance) {
  useLook.setState({ appearance: a, look: normaliseLook(a.look) });
  apply();
}

let saveTimer: ReturnType<typeof setTimeout> | undefined;

/// Shows an appearance at once and saves it shortly after (sliders and drags change it many
/// times a second).
export function changeAppearance(next: Appearance) {
  applyAppearance(next);
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => void attempt(api.setAppearance(next)), 300);
}

/// Changes part of the look, from the latest state (not a render's copy).
export function updateLook(patch: Partial<Look>) {
  const { appearance, look } = useLook.getState();
  changeAppearance({ ...appearance, look: { ...look, ...patch } });
}

export function updateLayout(patch: Partial<Layout>) {
  updateLook({ layout: { ...useLook.getState().look.layout, ...patch } });
}

/// Shows a theme in place of the chosen one while it's edited; `null` ends the preview.
export function previewTheme(t: ThemeDef | null) {
  useLook.setState({ preview: t });
  apply();
}

export async function reloadThemes() {
  const saved = (await api.themes()).map(toThemeDef);
  useLook.setState({ saved });
  apply();
}

/// Loads the saved appearance and themes, follows changes made in any window or on the desktop,
/// and follows the artwork of `artHash` when the look uses artwork colours.
export function useAppearance(artHash: string | null = null) {
  useEffect(() => {
    void api.settings().then((s) => applyAppearance(s.appearance));
    void reloadThemes();
    const unlisten = [onAppearance(applyAppearance), onThemesChanged(() => void reloadThemes())];
    const media = [matchMedia(motionQuery), matchMedia(transparencyQuery), matchMedia(darkQuery)];
    media.forEach((m) => m.addEventListener('change', apply));
    return () => {
      unlisten.forEach((u) => void u.then((f) => f()));
      media.forEach((m) => m.removeEventListener('change', apply));
    };
  }, []);

  const look = useLook((s) => s.look);
  const wantsArtwork = look.accentSource === 'artwork' || look.background === 'artwork';
  useEffect(() => {
    if (!wantsArtwork || !artHash) {
      useLook.setState({ palette: [] });
      apply();
      return;
    }
    let live = true;
    api.artworkPalette(artHash).then(
      (colors) => {
        if (!live) return;
        useLook.setState({ palette: colors });
        apply();
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [wantsArtwork, artHash]);
}
