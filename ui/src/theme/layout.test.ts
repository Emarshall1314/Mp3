import { describe, expect, it } from 'vitest';
import {
  BUILT_IN_PRESETS,
  DEFAULT_LAYOUT,
  columnTemplate,
  move,
  moveTo,
  normaliseLayout,
  normalisePresets,
} from './layout';

describe('layout', () => {
  it('fills in defaults and drops what it does not know', () => {
    expect(normaliseLayout(undefined)).toEqual(DEFAULT_LAYOUT);
    const l = normaliseLayout({
      nav: ['songs', 'bogus', 'songs', 'home'],
      sidebarWidth: 9000,
      playerPosition: 'left',
      columns: [
        { id: 'artist', width: 10 },
        { id: 'nope', width: 50 },
        { id: 'artist', width: 90 },
      ],
      views: { albums: 'list' },
    });
    expect(l.nav).toEqual(['songs', 'home']);
    expect(l.sidebarWidth).toBe(400);
    expect(l.playerPosition).toBe('bottom');
    expect(l.views).toEqual({ albums: 'list', recent: 'grid', artists: 'list' });
    // The title column always stays; widths are clamped; duplicates dropped.
    expect(l.columns).toEqual([
      { id: 'title', width: 220 },
      { id: 'artist', width: 40 },
    ]);
  });

  it('keeps presets that make sense', () => {
    const p = normalisePresets([
      { name: ' Party ', layout: { nowPlaying: 'minimal' } },
      { name: '' },
      3,
    ]);
    expect(p).toHaveLength(1);
    expect(p[0]?.name).toBe('Party');
    expect(p[0]?.layout.nowPlaying).toBe('minimal');
    for (const b of BUILT_IN_PRESETS) expect(normaliseLayout(b.layout)).toEqual(b.layout);
  });

  it('builds the column template and reorders lists', () => {
    expect(
      columnTemplate([
        { id: 'title', width: 200 },
        { id: 'time', width: 60 },
      ]),
    ).toBe('minmax(120px, 1fr) minmax(40px, 60px)');
    expect(move(['a', 'b', 'c'], 0, -1)).toEqual(['a', 'b', 'c']);
    expect(move(['a', 'b', 'c'], 0, 1)).toEqual(['b', 'a', 'c']);
    expect(moveTo(['a', 'b', 'c', 'd'], 0, 2)).toEqual(['b', 'c', 'a', 'd']);
    expect(moveTo(['a', 'b', 'c', 'd'], 3, 1)).toEqual(['a', 'd', 'b', 'c']);
  });
  it('keeps the phone tabs usable', () => {
    expect(normaliseLayout({}).phoneTabs).toEqual(['home', 'library', 'search', 'settings']);
    // Library is always there, and at most five tabs fit.
    expect(normaliseLayout({ phoneTabs: ['songs', 'home'] }).phoneTabs).toEqual([
      'library',
      'songs',
      'home',
    ]);
    expect(
      normaliseLayout({
        phoneTabs: ['home', 'library', 'search', 'albums', 'artists', 'songs', 'sync'],
      }).phoneTabs,
    ).toHaveLength(5);
    expect(normaliseLayout({ phoneTabs: ['nonsense', 'home', 'home'] }).phoneTabs).toEqual([
      'library',
      'home',
    ]);
    expect(normaliseLayout({ miniPlayer: 'bar' }).miniPlayer).toBe('bar');
    expect(normaliseLayout({ miniPlayer: 'huge' }).miniPlayer).toBe('floating');
  });
});
