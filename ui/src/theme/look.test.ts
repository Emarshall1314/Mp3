import { describe, expect, it } from 'vitest';
import {
  BUILT_IN_THEMES,
  DEFAULT_LOOK,
  THEME_TOKENS,
  accentFromArtwork,
  buildLook,
  contrast,
  contrastChecks,
  normaliseLook,
  parseColor,
  themeFor,
} from './look';

describe('theme engine', () => {
  it('parses the colour forms themes use', () => {
    expect(parseColor('#fff')).toEqual([255, 255, 255, 1]);
    expect(parseColor('#00000080')?.[3]).toBeCloseTo(0.5, 2);
    expect(parseColor('rgb(0 0 0 / 10%)')).toEqual([0, 0, 0, 0.1]);
    expect(parseColor('rgba(10, 20, 30, 0.5)')).toEqual([10, 20, 30, 0.5]);
    expect(parseColor('linear-gradient(red, blue)')).toBeNull();
  });

  it('has eight built-in themes that all meet WCAG AA and set every token', () => {
    expect(BUILT_IN_THEMES).toHaveLength(8);
    for (const t of BUILT_IN_THEMES) {
      for (const token of THEME_TOKENS)
        expect(t.tokens[token.name], `${t.id} ${token.name}`).toBeTruthy();
      const checks = contrastChecks(t.tokens);
      expect(checks.length).toBe(5);
      for (const c of checks)
        expect(c.ratio, `${t.name}: ${c.label}`).toBeGreaterThanOrEqual(c.needed);
    }
  });

  it('warns about unreadable custom themes', () => {
    const checks = contrastChecks({ 'color-bg': '#ffffff', 'color-text': '#dddddd' });
    expect(checks[0]?.ratio).toBeLessThan(4.5);
  });

  it('fills in defaults and clamps stored looks', () => {
    expect(normaliseLook(null)).toEqual(DEFAULT_LOOK);
    const l = normaliseLook({
      blur: 500,
      density: 'huge',
      customFont: 'Fira Sans"; x',
      accent: 'nope',
    });
    expect(l.blur).toBe(60);
    expect(l.density).toBe('comfortable');
    expect(l.customFont).toBe('Fira Sans x');
    expect(l.accent).toBe(DEFAULT_LOOK.accent);
  });

  it('builds tokens for accent, background, panels and density', () => {
    const dark = themeFor({ ...DEFAULT_LOOK, darkTheme: 'midnight' }, 'dark', []);
    expect(dark.id).toBe('midnight');
    const { vars, attrs } = buildLook(
      {
        ...DEFAULT_LOOK,
        accentSource: 'custom',
        accent: '#2f7cf6',
        background: 'gradient',
        density: 'compact',
        radius: 50,
      },
      dark,
      [],
      false,
    );
    expect(vars['--color-accent']).toBe('#2f7cf6');
    expect(vars['--app-background']).toContain('linear-gradient');
    expect(vars['--row-height']).toBe('28px');
    expect(vars['--radius-m']).toBe('5px');
    expect(attrs['data-density']).toBe('compact');

    const solid = buildLook({ ...DEFAULT_LOOK, background: 'artwork' }, dark, [], true);
    expect(solid.vars['--blur-glass']).toBe('0px');
    expect(solid.attrs['data-panels']).toBe('solid');
    // No artwork: the artwork background falls back to the theme's gradient.
    expect(solid.attrs['data-background']).toBe('gradient');
  });

  it('keeps artwork accents readable', () => {
    const white: [number, number, number] = [255, 255, 255];
    const a = accentFromArtwork(
      [
        [250, 240, 120],
        [200, 200, 200],
      ],
      white,
    );
    expect(a && contrast(a, white)).toBeGreaterThanOrEqual(3);
    expect(themeFor({ ...DEFAULT_LOOK, lightTheme: 'gone' }, 'light', []).id).toBe('default-light');
  });
});
