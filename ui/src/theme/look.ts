import {
  type Layout,
  type Preset,
  DEFAULT_LAYOUT,
  PLAYER_HEIGHTS,
  normaliseLayout,
  normalisePresets,
} from './layout';

// The theme engine: built-in themes, the owner's look settings, and the CSS custom properties
// they produce. Pure functions; useAppearance puts the result on <html>.

export type Scheme = 'light' | 'dark';

export interface ThemeDef {
  id: string;
  name: string;
  scheme: Scheme;
  /// CSS custom properties without the leading `--`.
  tokens: Record<string, string>;
  builtIn?: boolean;
}

export type AccentSource = 'theme' | 'custom' | 'artwork';
export type BackgroundStyle = 'solid' | 'gradient' | 'artwork';
export type PanelStyle = 'glass' | 'liquid' | 'solid';
export type Density = 'compact' | 'comfortable' | 'spacious';
export type FontChoice = 'inter' | 'system' | 'serif' | 'rounded' | 'mono' | 'custom';
export type ArtworkSize = 'small' | 'medium' | 'large';

/// Everything about the look that isn't the theme's own tokens. Stored in the appearance
/// settings (`look`), shared by every window.
export interface Look {
  lightTheme: string;
  darkTheme: string;
  accentSource: AccentSource;
  accent: string;
  background: BackgroundStyle;
  /// Sidebar, player bar, menus: frosted glass, liquid glass, or solid.
  panels: PanelStyle;
  /// Blur behind glass panels, in px.
  blur: number;
  /// How see-through glass panels are, 0–90 %.
  transparency: number;
  /// Corner rounding, % of the theme's.
  radius: number;
  density: Density;
  font: FontChoice;
  customFont: string;
  /// Text size, % of normal.
  fontSize: number;
  artworkSize: ArtworkSize;
  layout: Layout;
  /// The owner's named layouts (built-in ones are in theme/layout).
  presets: Preset[];
  /// Remapped shortcuts: action id → key combination ('' for none).
  shortcuts: Record<string, string>;
  /// The owner's CSS, applied after the theme (desktop; ignored in safe mode).
  customCss: string;
}

export const DEFAULT_LOOK: Look = {
  lightTheme: 'default-light',
  darkTheme: 'default-dark',
  accentSource: 'theme',
  accent: '#e8344e',
  background: 'solid',
  panels: 'glass',
  blur: 30,
  transparency: 28,
  radius: 100,
  density: 'comfortable',
  font: 'inter',
  customFont: '',
  fontSize: 100,
  artworkSize: 'medium',
  layout: DEFAULT_LAYOUT,
  presets: [],
  shortcuts: {},
  customCss: '',
};

export const ACCENT_PRESETS = [
  { name: 'Red', color: '#e8344e' },
  { name: 'Orange', color: '#f0731f' },
  { name: 'Amber', color: '#e0a800' },
  { name: 'Green', color: '#2fa84f' },
  { name: 'Teal', color: '#1a9e9a' },
  { name: 'Blue', color: '#2f7cf6' },
  { name: 'Indigo', color: '#5b5bd6' },
  { name: 'Purple', color: '#9b51e0' },
  { name: 'Pink', color: '#e0458f' },
  { name: 'Graphite', color: '#7c7c83' },
];

/// The tokens a theme sets, grouped for the theme editor.
export const THEME_TOKENS: {
  name: string;
  label: string;
  group: string;
  kind: 'color' | 'text';
}[] = [
  { name: 'color-bg', label: 'Background', group: 'Colours', kind: 'color' },
  { name: 'color-surface', label: 'Panels', group: 'Colours', kind: 'color' },
  { name: 'color-text', label: 'Text', group: 'Colours', kind: 'color' },
  { name: 'color-text-secondary', label: 'Secondary text', group: 'Colours', kind: 'color' },
  { name: 'color-accent', label: 'Accent', group: 'Colours', kind: 'color' },
  { name: 'color-accent-text', label: 'Text on accent', group: 'Colours', kind: 'color' },
  { name: 'color-separator', label: 'Separators', group: 'Colours', kind: 'color' },
  { name: 'color-hover', label: 'Hover', group: 'Colours', kind: 'color' },
  { name: 'color-selected', label: 'Selection', group: 'Colours', kind: 'color' },
  { name: 'color-placeholder', label: 'Artwork placeholder', group: 'Colours', kind: 'color' },
  { name: 'color-badge', label: 'Badges', group: 'Colours', kind: 'color' },
  { name: 'background-gradient', label: 'Gradient', group: 'Background', kind: 'text' },
  { name: 'shadow-artwork', label: 'Artwork shadow', group: 'Shadows', kind: 'text' },
  { name: 'shadow-menu', label: 'Menu shadow', group: 'Shadows', kind: 'text' },
  { name: 'radius-s', label: 'Small corners', group: 'Shape and type', kind: 'text' },
  { name: 'radius-m', label: 'Medium corners', group: 'Shape and type', kind: 'text' },
  { name: 'radius-l', label: 'Large corners', group: 'Shape and type', kind: 'text' },
  { name: 'font-sans', label: 'Font', group: 'Shape and type', kind: 'text' },
];

const shape = {
  'radius-s': '6px',
  'radius-m': '10px',
  'radius-l': '16px',
  'font-sans': "'Inter Variable', 'Inter', system-ui, sans-serif",
};

const lightShadows = {
  'shadow-artwork': '0 4px 16px rgb(0 0 0 / 14%)',
  'shadow-menu': '0 8px 28px rgb(0 0 0 / 18%)',
};

const darkShadows = {
  'shadow-artwork': '0 4px 16px rgb(0 0 0 / 45%)',
  'shadow-menu': '0 8px 28px rgb(0 0 0 / 50%)',
};

const lightTints = {
  'color-separator': 'rgb(0 0 0 / 10%)',
  'color-hover': 'rgb(0 0 0 / 5%)',
  'color-selected': 'rgb(0 0 0 / 8%)',
  'color-badge': 'rgb(0 0 0 / 7%)',
};

const darkTints = {
  'color-separator': 'rgb(255 255 255 / 12%)',
  'color-hover': 'rgb(255 255 255 / 6%)',
  'color-selected': 'rgb(255 255 255 / 10%)',
  'color-badge': 'rgb(255 255 255 / 10%)',
};

const theme = (
  id: string,
  name: string,
  scheme: Scheme,
  colors: Record<string, string>,
): ThemeDef => ({
  id,
  name,
  scheme,
  builtIn: true,
  tokens: {
    ...shape,
    ...(scheme === 'light' ? lightShadows : darkShadows),
    ...(scheme === 'light' ? lightTints : darkTints),
    ...colors,
  },
});

export const BUILT_IN_THEMES: ThemeDef[] = [
  theme('default-light', 'Default Light', 'light', {
    'color-bg': '#ffffff',
    'color-surface': '#f5f5f7',
    'color-text': '#1d1d1f',
    'color-text-secondary': '#6e6e73',
    'color-accent': '#d92d47',
    'color-accent-text': '#ffffff',
    'color-placeholder': '#e3e3e8',
    'background-gradient': 'linear-gradient(160deg, #ffffff 0%, #f6eef1 55%, #ece9f6 100%)',
  }),
  theme('default-dark', 'Default Dark', 'dark', {
    'color-bg': '#1c1c1e',
    'color-surface': '#242426',
    'color-text': '#f5f5f7',
    'color-text-secondary': '#98989d',
    'color-accent': '#ff4d63',
    'color-accent-text': '#1c1c1e',
    'color-placeholder': '#3a3a3c',
    'background-gradient': 'linear-gradient(160deg, #1c1c1e 0%, #2a1d24 55%, #1b1d2c 100%)',
  }),
  theme('oled-black', 'OLED Black', 'dark', {
    'color-bg': '#000000',
    'color-surface': '#0d0d0f',
    'color-text': '#f5f5f7',
    'color-text-secondary': '#8e8e93',
    'color-accent': '#ff4d63',
    'color-accent-text': '#000000',
    'color-placeholder': '#1c1c1e',
    'background-gradient': 'linear-gradient(180deg, #000000 0%, #0b0b10 100%)',
    'shadow-artwork': 'none',
  }),
  theme('high-contrast', 'High Contrast', 'dark', {
    'color-bg': '#000000',
    'color-surface': '#121212',
    'color-text': '#ffffff',
    'color-text-secondary': '#e2e2e2',
    'color-accent': '#ffd60a',
    'color-accent-text': '#000000',
    'color-separator': 'rgb(255 255 255 / 55%)',
    'color-hover': 'rgb(255 255 255 / 16%)',
    'color-selected': 'rgb(255 255 255 / 26%)',
    'color-placeholder': '#2a2a2a',
    'color-badge': 'rgb(255 255 255 / 22%)',
    'background-gradient': 'linear-gradient(180deg, #000000 0%, #000000 100%)',
  }),
  theme('paper', 'Paper', 'light', {
    'color-bg': '#f7f2e8',
    'color-surface': '#efe7d7',
    'color-text': '#2b2620',
    'color-text-secondary': '#685e52',
    'color-accent': '#a8461f',
    'color-accent-text': '#ffffff',
    'color-separator': 'rgb(60 40 20 / 14%)',
    'color-hover': 'rgb(60 40 20 / 6%)',
    'color-selected': 'rgb(60 40 20 / 10%)',
    'color-placeholder': '#e4d9c4',
    'color-badge': 'rgb(60 40 20 / 8%)',
    'background-gradient': 'linear-gradient(170deg, #f9f4eb 0%, #efe4cf 100%)',
    'font-sans': "'Iowan Old Style', 'Palatino Linotype', 'Noto Serif', Georgia, serif",
  }),
  theme('midnight', 'Midnight', 'dark', {
    'color-bg': '#0f1424',
    'color-surface': '#161d33',
    'color-text': '#e8ecf8',
    'color-text-secondary': '#9aa4c2',
    'color-accent': '#7d95ff',
    'color-accent-text': '#0b1020',
    'color-placeholder': '#252e4a',
    'background-gradient': 'linear-gradient(160deg, #0f1424 0%, #1d1640 55%, #0b2238 100%)',
  }),
  theme('forest', 'Forest', 'dark', {
    'color-bg': '#101914',
    'color-surface': '#17231c',
    'color-text': '#e7efe9',
    'color-text-secondary': '#98ab9f',
    'color-accent': '#5fcf8a',
    'color-accent-text': '#062312',
    'color-placeholder': '#24352b',
    'background-gradient': 'linear-gradient(160deg, #0f1a14 0%, #1c2b1a 55%, #0e1f1d 100%)',
  }),
  theme('sunset', 'Sunset', 'light', {
    'color-bg': '#fff7f0',
    'color-surface': '#fde9dc',
    'color-text': '#2a1712',
    'color-text-secondary': '#77574c',
    'color-accent': '#c33d17',
    'color-accent-text': '#ffffff',
    'color-separator': 'rgb(90 30 10 / 12%)',
    'color-hover': 'rgb(90 30 10 / 5%)',
    'color-selected': 'rgb(90 30 10 / 9%)',
    'color-placeholder': '#f5d6c4',
    'color-badge': 'rgb(90 30 10 / 8%)',
    'background-gradient': 'linear-gradient(160deg, #ffe8d6 0%, #ffd6e0 50%, #eadbff 100%)',
  }),
];

/// Fills in defaults for anything missing or out of range (older settings, hand-edited files).
export function normaliseLook(value: unknown): Look {
  const v = (value && typeof value === 'object' ? value : {}) as Partial<
    Record<keyof Look, unknown>
  >;
  const pick = <T extends string>(x: unknown, options: readonly T[], fallback: T): T =>
    options.includes(x as T) ? (x as T) : fallback;
  const num = (x: unknown, lo: number, hi: number, fallback: number) =>
    typeof x === 'number' && Number.isFinite(x) ? Math.min(hi, Math.max(lo, x)) : fallback;
  const str = (x: unknown, fallback: string) => (typeof x === 'string' && x ? x : fallback);
  const d = DEFAULT_LOOK;
  return {
    lightTheme: str(v.lightTheme, d.lightTheme),
    darkTheme: str(v.darkTheme, d.darkTheme),
    accentSource: pick(v.accentSource, ['theme', 'custom', 'artwork'], d.accentSource),
    accent: parseColor(str(v.accent, '')) ? str(v.accent, d.accent) : d.accent,
    background: pick(v.background, ['solid', 'gradient', 'artwork'], d.background),
    panels: pick(v.panels, ['glass', 'liquid', 'solid'], d.panels),
    blur: num(v.blur, 0, 60, d.blur),
    transparency: num(v.transparency, 0, 90, d.transparency),
    radius: num(v.radius, 0, 200, d.radius),
    density: pick(v.density, ['compact', 'comfortable', 'spacious'], d.density),
    font: pick(v.font, ['inter', 'system', 'serif', 'rounded', 'mono', 'custom'], d.font),
    customFont: safeFontName(str(v.customFont, '')),
    fontSize: num(v.fontSize, 80, 140, d.fontSize),
    artworkSize: pick(v.artworkSize, ['small', 'medium', 'large'], d.artworkSize),
    layout: normaliseLayout(v.layout),
    presets: normalisePresets(v.presets),
    shortcuts: Object.fromEntries(
      Object.entries(v.shortcuts && typeof v.shortcuts === 'object' ? v.shortcuts : {})
        .filter(([k, s]) => /^[a-zA-Z]{1,40}$/.test(k) && typeof s === 'string' && s.length <= 40)
        .slice(0, 100),
    ) as Record<string, string>,
    customCss: typeof v.customCss === 'string' ? v.customCss.slice(0, 100_000) : '',
  };
}

/// Font names are limited to what a font family name can hold, so they can't break the CSS.
export const safeFontName = (name: string) => name.replace(/[^\p{L}\p{N} _.-]/gu, '').slice(0, 60);

export const FONT_STACKS: Record<Exclude<FontChoice, 'custom'>, string> = {
  inter: "'Inter Variable', 'Inter', system-ui, sans-serif",
  system: 'system-ui, -apple-system, Cantarell, Ubuntu, Roboto, sans-serif',
  serif: "'Iowan Old Style', 'Palatino Linotype', 'Noto Serif', 'DejaVu Serif', Georgia, serif",
  rounded: "'Nunito', 'Quicksand', 'Varela Round', 'Comfortaa', system-ui, sans-serif",
  mono: "'JetBrains Mono', 'Fira Code', 'DejaVu Sans Mono', ui-monospace, monospace",
};

export const ARTWORK_SIZES: Record<ArtworkSize, number> = { small: 200, medium: 260, large: 340 };

export const DENSITY: Record<Density, { space: number; row: number }> = {
  compact: { space: 0.75, row: 28 },
  comfortable: { space: 1, row: 34 },
  spacious: { space: 1.25, row: 42 },
};

export type Rgb = [number, number, number];
export type Rgba = [number, number, number, number];

/// A CSS colour as RGBA (0–255, alpha 0–1): hex and rgb()/rgba() here, anything else through
/// the browser when it's available. `null` when it isn't a single colour.
export function parseColor(css: string): Rgba | null {
  const s = css.trim().toLowerCase();
  const hex = /^#([0-9a-f]{3,8})$/.exec(s)?.[1];
  if (hex && [3, 4, 6, 8].includes(hex.length)) {
    const full = hex.length <= 4 ? [...hex].map((c) => c + c).join('') : hex;
    const n = (i: number) => parseInt(full.slice(i, i + 2), 16);
    return [n(0), n(2), n(4), full.length === 8 ? n(6) / 255 : 1];
  }
  const rgb = /^rgba?\(([^)]*)\)$/.exec(s)?.[1];
  if (rgb) {
    const parts = rgb.split(/[\s,/]+/).filter(Boolean);
    if (parts.length < 3) return null;
    const channel = (p: string) => (p.endsWith('%') ? (parseFloat(p) * 255) / 100 : parseFloat(p));
    const alpha =
      parts[3] === undefined
        ? 1
        : parts[3].endsWith('%')
          ? parseFloat(parts[3]) / 100
          : parseFloat(parts[3]);
    const [r = '', g = '', b = ''] = parts;
    const out: Rgba = [channel(r), channel(g), channel(b), alpha];
    return out.every((x) => Number.isFinite(x)) ? out : null;
  }
  return browserColor(s);
}

let probe: CanvasRenderingContext2D | null | undefined;
function browserColor(s: string): Rgba | null {
  if (
    typeof document === 'undefined' ||
    /jsdom/.test(navigator.userAgent) ||
    /gradient|var\(|url\(/.test(s)
  )
    return null;
  try {
    probe ??= document.createElement('canvas').getContext('2d');
  } catch {
    probe = null;
  }
  if (!probe) return null;
  // An invalid colour leaves fillStyle unchanged: check against two different starting values.
  probe.fillStyle = '#010203';
  probe.fillStyle = s;
  const a = probe.fillStyle;
  probe.fillStyle = '#040506';
  probe.fillStyle = s;
  if (a !== probe.fillStyle) return null;
  return a.startsWith('#') || a.startsWith('rgb') ? parseColor(a) : null;
}

const hexOf = ([r, g, b]: Rgb | Rgba) =>
  '#' +
  [r, g, b]
    .map((c) =>
      Math.round(Math.min(255, Math.max(0, c)))
        .toString(16)
        .padStart(2, '0'),
    )
    .join('');

/// A colour laid over another (for translucent colours).
export function over(top: Rgba, below: Rgb): Rgb {
  const a = top[3];
  return [0, 1, 2].map((i) => (top[i] ?? 0) * a + (below[i] ?? 0) * (1 - a)) as Rgb;
}

function luminance([r, g, b]: Rgb): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/// WCAG contrast ratio, 1–21.
export function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

export interface ContrastCheck {
  label: string;
  ratio: number;
  needed: number;
}

/// The text/background pairs a theme must keep readable (WCAG AA: 4.5 for text, 3 for large
/// text and icons). Pairs whose colours can't be read (gradients, variables) are skipped.
export function contrastChecks(tokens: Record<string, string>): ContrastCheck[] {
  const color = (name: string) => parseColor(tokens[name] ?? '');
  const bg = color('color-bg');
  if (!bg) return [];
  const bgRgb = over(bg, [255, 255, 255]);
  const pairs: [string, string, string, number][] = [
    ['Text on background', 'color-text', 'color-bg', 4.5],
    ['Text on panels', 'color-text', 'color-surface', 4.5],
    ['Secondary text on background', 'color-text-secondary', 'color-bg', 4.5],
    ['Text on accent', 'color-accent-text', 'color-accent', 4.5],
    ['Accent on background (links, icons)', 'color-accent', 'color-bg', 3],
  ];
  const out: ContrastCheck[] = [];
  for (const [label, fg, back, needed] of pairs) {
    const f = color(fg);
    const b = color(back);
    if (!f || !b) continue;
    const below = over(b, bgRgb);
    out.push({ label, ratio: contrast(over(f, below), below), needed });
  }
  return out;
}

/// Black or white, whichever reads better on `c`.
export function textOn(c: Rgb): string {
  return contrast(c, [0, 0, 0]) >= contrast(c, [255, 255, 255]) ? '#000000' : '#ffffff';
}

/// The most colourful of the artwork's colours, lightened or darkened until it stands out
/// against the background (3:1).
export function accentFromArtwork(palette: Rgb[], bg: Rgb): Rgb | null {
  if (!palette.length) return null;
  const chroma = ([r, g, b]: Rgb) => Math.max(r, g, b) - Math.min(r, g, b);
  let c = [...palette].sort((a, b) => chroma(b) - chroma(a))[0] ?? palette[0];
  if (!c) return null;
  const towards: Rgb = luminance(bg) > 0.4 ? [0, 0, 0] : [255, 255, 255];
  for (let i = 0; i < 20 && contrast(c, bg) < 3; i++)
    c = c.map((x, k) => x + ((towards[k] ?? x) - x) * 0.12) as Rgb;
  return c;
}

/// Every CSS custom property and <html> attribute for a look with a theme. `palette` is the
/// current artwork's colours (artwork accent and background).
export function buildLook(
  look: Look,
  theme: ThemeDef,
  palette: Rgb[],
  reduceTransparency: boolean,
): { vars: Record<string, string>; attrs: Record<string, string> } {
  const t = theme.tokens;
  const vars: Record<string, string> = {};
  for (const [k, v] of Object.entries(t)) vars[`--${k}`] = v;
  const bgParsed = parseColor(t['color-bg'] ?? '') ?? [255, 255, 255, 1];
  const bg = over(bgParsed, theme.scheme === 'dark' ? [0, 0, 0] : [255, 255, 255]);

  // Accent.
  let accent: Rgb | null = null;
  if (look.accentSource === 'custom') {
    const c = parseColor(look.accent);
    if (c) accent = over(c, bg);
  } else if (look.accentSource === 'artwork') accent = accentFromArtwork(palette, bg);
  if (accent) {
    vars['--color-accent'] = hexOf(accent);
    vars['--color-accent-text'] = textOn(accent);
  }

  // Background behind everything.
  const gradient = t['background-gradient'] ?? t['color-bg'] ?? '#ffffff';
  let background = 'var(--color-bg)';
  let backgroundStyle: BackgroundStyle = look.background;
  if (look.background === 'gradient') background = gradient;
  else if (look.background === 'artwork') {
    if (palette.length) {
      const a = theme.scheme === 'dark' ? 0.42 : 0.3;
      const spots = ['15% 10%', '85% 20%', '30% 95%', '90% 90%'];
      background = [
        ...palette
          .slice(0, 4)
          .map(
            (c, i) =>
              `radial-gradient(at ${spots[i]}, rgb(${c.join(' ')} / ${a}), transparent 62%)`,
          ),
        hexOf(bg),
      ].join(', ');
    } else {
      background = gradient;
      backgroundStyle = 'gradient';
    }
  }
  vars['--app-background'] = background;

  // Panels.
  const surface = over(parseColor(t['color-surface'] ?? '') ?? [245, 245, 247, 1], bg);
  const panels: PanelStyle = reduceTransparency ? 'solid' : look.panels;
  if (panels === 'solid') {
    vars['--color-surface-glass'] = hexOf(surface);
    vars['--blur-glass'] = '0px';
  } else {
    const alpha = 1 - look.transparency / 100;
    vars['--color-surface-glass'] =
      `rgb(${surface.map(Math.round).join(' ')} / ${Math.round(alpha * 100)}%)`;
    vars['--blur-glass'] = `${look.blur}px`;
  }

  // Shape, density, type.
  for (const r of ['radius-s', 'radius-m', 'radius-l']) {
    const px = parseFloat(t[r] ?? '');
    if (Number.isFinite(px)) vars[`--${r}`] = `${Math.round(px * look.radius) / 100}px`;
  }
  const d = DENSITY[look.density];
  for (const [name, px] of [
    ['space-1', 4],
    ['space-2', 8],
    ['space-3', 12],
    ['space-4', 16],
    ['space-6', 24],
    ['space-8', 32],
  ] as const)
    vars[`--${name}`] = `${Math.round(px * d.space)}px`;
  vars['--row-height'] = `${d.row}px`;
  if (look.font === 'custom' && look.customFont)
    vars['--font-sans'] = `'${look.customFont}', ${FONT_STACKS.inter}`;
  else if (look.font !== 'custom' && look.font !== 'inter')
    vars['--font-sans'] = FONT_STACKS[look.font];
  vars['--np-art-scale'] = String(ARTWORK_SIZES[look.artworkSize] / ARTWORK_SIZES.medium);

  // Layout sizes.
  const layout = look.layout;
  vars['--sidebar-width'] = layout.sidebarCollapsed ? '64px' : `${layout.sidebarWidth}px`;
  vars['--player-height'] = `${PLAYER_HEIGHTS[layout.playerSize]}px`;

  return {
    vars,
    attrs: {
      'data-scheme': theme.scheme,
      'data-background': backgroundStyle,
      'data-panels': panels,
      'data-density': look.density,
      'data-sidebar': layout.sidebarCollapsed ? 'collapsed' : 'open',
      'data-player': `${layout.playerPosition} ${layout.playerSize}`,
    },
  };
}

/// The theme for a scheme: the chosen one, or the default when it no longer exists.
export function themeFor(look: Look, scheme: Scheme, saved: ThemeDef[]): ThemeDef {
  const id = scheme === 'dark' ? look.darkTheme : look.lightTheme;
  return [...BUILT_IN_THEMES, ...saved].find((t) => t.id === id) ?? defaultTheme(scheme);
}

export const defaultTheme = (scheme: Scheme): ThemeDef =>
  (scheme === 'dark' ? BUILT_IN_THEMES[1] : BUILT_IN_THEMES[0]) as ThemeDef;
