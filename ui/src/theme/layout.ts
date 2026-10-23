// The layout: what the sidebar lists and in which order, the player bar, each section's default
// view, the song list's columns, the Now Playing style, and named presets of all of these.
// Stored in the look (appearance settings); pure functions here.

import type { SongSort } from '../types';

export type NavId = 'home' | 'recent' | 'artists' | 'albums' | 'songs';
export type PlayerControl = 'shuffle' | 'repeat' | 'volume' | 'output' | 'miniPlayer' | 'queue';
export type ColumnId =
  | 'title'
  | 'artist'
  | 'album'
  | 'albumartist'
  | 'track'
  | 'year'
  | 'genre'
  | 'format'
  | 'quality'
  | 'plays'
  | 'time';
export type Grouping = 'none' | 'album' | 'artist' | 'genre';
export type ShelfId = 'rediscover' | 'recentlyPlayed' | 'recentlyAdded' | 'mostPlayed';
export const ALL_SHELVES: readonly ShelfId[] = [
  'rediscover',
  'recentlyPlayed',
  'recentlyAdded',
  'mostPlayed',
];
/// The shelves there were before Rediscover; a saved layout from then lists only these.
const FIRST_SHELVES: readonly ShelfId[] = ['recentlyPlayed', 'recentlyAdded', 'mostPlayed'];
export type NowPlayingStyle = 'artwork' | 'lyrics' | 'minimal' | 'visualizer';
/// The phone's tab bar.
export type TabId =
  'home' | 'library' | 'search' | 'albums' | 'artists' | 'songs' | 'recent' | 'sync' | 'settings';
/// The phone's player above the tabs: a floating card, a full-width bar, or just the essentials.
export type MiniPlayerStyle = 'floating' | 'bar' | 'minimal';

export interface Column {
  id: ColumnId;
  /// Width in px (the title column takes what's left instead).
  width: number;
}

export interface Dock {
  position: 'right' | 'left' | 'bottom' | 'floating';
  /// Width (left, right) or height (bottom), px.
  size: number;
  /// Where it floats: px from the workspace's top left, and its size.
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Layout {
  /// Sidebar library items in order; ones not listed are hidden.
  nav: NavId[];
  sidebarWidth: number;
  /// Icons only.
  sidebarCollapsed: boolean;
  playerPosition: 'top' | 'bottom';
  playerSize: 'compact' | 'standard' | 'large';
  playerControls: PlayerControl[];
  columns: Column[];
  nowPlaying: NowPlayingStyle;
  /// The panel holding the queue, lyrics, info and signal path.
  dock: Dock;
  /// Home's shelves in order; ones not listed are hidden.
  shelves: ShelfId[];
  /// Every shelf this layout has been offered, so a shelf added in a later version appears
  /// once (at the top) instead of counting as hidden.
  knownShelves: ShelfId[];
  /// The song list grouped (and sorted) by album, artist or genre.
  grouping: Grouping;
  /// Song Info opens in the side panel instead of over the page.
  tagEditorDocked: boolean;
  /// The phone's tabs in order (at most five).
  phoneTabs: TabId[];
  miniPlayer: MiniPlayerStyle;
}

export interface Preset {
  name: string;
  layout: Layout;
}

export const NAV_LABELS: Record<NavId, string> = {
  home: 'Home',
  recent: 'Recently Added',
  artists: 'Artists',
  albums: 'Albums',
  songs: 'Songs',
};

export const SHELF_LABELS: Record<ShelfId, string> = {
  rediscover: 'Rediscover',
  recentlyPlayed: 'Recently Played',
  recentlyAdded: 'Recently Added',
  mostPlayed: 'Most Played',
};

export const TAB_LABELS: Record<TabId, string> = {
  home: 'Home',
  library: 'Library',
  search: 'Search',
  albums: 'Albums',
  artists: 'Artists',
  songs: 'Songs',
  recent: 'Recently Added',
  sync: 'Sync',
  settings: 'Settings',
};

export const MINI_PLAYER_LABELS: Record<MiniPlayerStyle, string> = {
  floating: 'Floating',
  bar: 'Bar',
  minimal: 'Minimal',
};

/// Phone tabs: at most this many fit across a phone.
export const MAX_TABS = 5;
const ALL_TABS = Object.keys(TAB_LABELS) as TabId[];

export const CONTROL_LABELS: Record<PlayerControl, string> = {
  shuffle: 'Shuffle',
  repeat: 'Repeat',
  volume: 'Volume',
  output: 'Output picker',
  miniPlayer: 'Mini player',
  queue: 'Queue',
};

/// Every column the song list can show: label, sort key (if it sorts) and default width.
export const COLUMN_SPECS: Record<
  ColumnId,
  { label: string; sort: SongSort | null; width: number }
> = {
  title: { label: 'Title', sort: 'title', width: 220 },
  artist: { label: 'Artist', sort: 'artist', width: 180 },
  album: { label: 'Album', sort: 'album', width: 180 },
  albumartist: { label: 'Album Artist', sort: null, width: 160 },
  track: { label: 'Track', sort: null, width: 56 },
  year: { label: 'Year', sort: 'year', width: 56 },
  genre: { label: 'Genre', sort: 'genre', width: 110 },
  format: { label: 'Format', sort: null, width: 72 },
  quality: { label: 'Quality', sort: null, width: 112 },
  plays: { label: 'Plays', sort: 'plays', width: 52 },
  time: { label: 'Time', sort: 'duration', width: 60 },
};

const ALL_NAV: NavId[] = ['home', 'recent', 'artists', 'albums', 'songs'];
const ALL_CONTROLS: PlayerControl[] = [
  'shuffle',
  'repeat',
  'volume',
  'output',
  'miniPlayer',
  'queue',
];
const col = (id: ColumnId, width = COLUMN_SPECS[id].width): Column => ({ id, width });

export const DEFAULT_LAYOUT: Layout = {
  nav: ALL_NAV,
  sidebarWidth: 236,
  sidebarCollapsed: false,
  playerPosition: 'bottom',
  playerSize: 'standard',
  playerControls: ALL_CONTROLS,
  columns: (
    ['title', 'artist', 'album', 'year', 'genre', 'quality', 'plays', 'time'] as ColumnId[]
  ).map((id) => col(id)),
  nowPlaying: 'artwork',
  dock: { position: 'right', size: 320, x: 80, y: 80, w: 340, h: 420 },
  shelves: ['rediscover', 'recentlyPlayed', 'recentlyAdded', 'mostPlayed'],
  knownShelves: [...ALL_SHELVES],
  grouping: 'none',
  tagEditorDocked: false,
  phoneTabs: ['home', 'library', 'search', 'settings'],
  miniPlayer: 'floating',
};

export const BUILT_IN_PRESETS: Preset[] = [
  { name: 'Listening', layout: DEFAULT_LAYOUT },
  {
    name: 'Tagging',
    layout: {
      ...DEFAULT_LAYOUT,
      nav: ['songs', 'albums', 'artists', 'recent', 'home'],
      playerSize: 'compact',
      tagEditorDocked: true,
      dock: { ...DEFAULT_LAYOUT.dock, size: 460 },
      columns: (
        [
          'title',
          'artist',
          'album',
          'albumartist',
          'track',
          'year',
          'genre',
          'format',
          'time',
        ] as ColumnId[]
      ).map((id) => col(id)),
    },
  },
  {
    name: 'Minimal',
    layout: {
      ...DEFAULT_LAYOUT,
      nav: ['home', 'albums', 'songs'],
      sidebarCollapsed: true,
      playerSize: 'compact',
      playerControls: ['volume', 'queue'],
      columns: (['title', 'artist', 'time'] as ColumnId[]).map((id) => col(id)),
      nowPlaying: 'minimal',
    },
  },
];

export const PLAYER_HEIGHTS = { compact: 60, standard: 76, large: 96 } as const;

/// Fills in defaults for anything missing or invalid.
export function normaliseLayout(value: unknown): Layout {
  const v = (value && typeof value === 'object' ? value : {}) as Partial<
    Record<keyof Layout, unknown>
  >;
  const d = DEFAULT_LAYOUT;
  const list = <T extends string>(x: unknown, allowed: readonly T[], fallback: T[]): T[] =>
    Array.isArray(x) ? [...new Set(x.filter((i): i is T => allowed.includes(i as T)))] : fallback;
  const pick = <T extends string>(x: unknown, options: readonly T[], fallback: T): T =>
    options.includes(x as T) ? (x as T) : fallback;
  const columns = Array.isArray(v.columns)
    ? v.columns
        .filter(
          (c): c is Column =>
            !!c &&
            typeof c === 'object' &&
            (c as Column).id in COLUMN_SPECS &&
            typeof (c as Column).width === 'number',
        )
        .filter((c, i, all) => all.findIndex((o) => o.id === c.id) === i)
        .map((c) => ({ id: c.id, width: Math.round(Math.min(600, Math.max(40, c.width))) }))
    : d.columns;
  return {
    nav: list(v.nav, ALL_NAV, d.nav),
    sidebarWidth:
      typeof v.sidebarWidth === 'number'
        ? Math.round(Math.min(400, Math.max(180, v.sidebarWidth)))
        : d.sidebarWidth,
    sidebarCollapsed:
      typeof v.sidebarCollapsed === 'boolean' ? v.sidebarCollapsed : d.sidebarCollapsed,
    playerPosition: pick(v.playerPosition, ['top', 'bottom'], d.playerPosition),
    playerSize: pick(v.playerSize, ['compact', 'standard', 'large'], d.playerSize),
    playerControls: list(v.playerControls, ALL_CONTROLS, d.playerControls),
    // The title column is always there: a list without it can't be read.
    columns: columns.some((c) => c.id === 'title') ? columns : [col('title'), ...columns],
    nowPlaying: pick(v.nowPlaying, ['artwork', 'lyrics', 'minimal', 'visualizer'], d.nowPlaying),
    dock: normaliseDock(v.dock),
    tagEditorDocked: typeof v.tagEditorDocked === 'boolean' ? v.tagEditorDocked : d.tagEditorDocked,
    grouping: pick(v.grouping, ['none', 'album', 'artist', 'genre'], d.grouping),
    ...shelves(
      list(v.shelves, ALL_SHELVES, d.shelves),
      list(v.knownShelves, ALL_SHELVES, Array.isArray(v.shelves) ? [...FIRST_SHELVES] : []),
    ),
    phoneTabs: phoneTabs(list(v.phoneTabs, ALL_TABS, d.phoneTabs)),
    miniPlayer: pick(v.miniPlayer, ['floating', 'bar', 'minimal'], d.miniPlayer),
  };
}

/// Shelves new since the layout was saved go at the top, once; after that, hiding one sticks.
function shelves(
  saved: ShelfId[],
  known: ShelfId[],
): { shelves: ShelfId[]; knownShelves: ShelfId[] } {
  const added = ALL_SHELVES.filter((s) => !known.includes(s) && !saved.includes(s));
  return { shelves: [...added, ...saved], knownShelves: [...ALL_SHELVES] };
}

/// Library is always a tab (everything else is reachable from it), and five at most fit.
function phoneTabs(tabs: TabId[]): TabId[] {
  const withLibrary = tabs.includes('library') ? tabs : ['library' as TabId, ...tabs];
  return withLibrary.slice(0, MAX_TABS);
}

function normaliseDock(value: unknown): Dock {
  const v = (value && typeof value === 'object' ? value : {}) as Partial<
    Record<keyof Dock, unknown>
  >;
  const d = DEFAULT_LAYOUT.dock;
  const num = (x: unknown, lo: number, hi: number, fallback: number) =>
    typeof x === 'number' && Number.isFinite(x)
      ? Math.round(Math.min(hi, Math.max(lo, x)))
      : fallback;
  return {
    position: (['right', 'left', 'bottom', 'floating'] as const).includes(
      v.position as Dock['position'],
    )
      ? (v.position as Dock['position'])
      : d.position,
    size: num(v.size, 200, 800, d.size),
    x: num(v.x, 0, 4000, d.x),
    y: num(v.y, 0, 4000, d.y),
    w: num(v.w, 240, 1200, d.w),
    h: num(v.h, 200, 1200, d.h),
  };
}

export function normalisePresets(value: unknown): Preset[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter(
      (p): p is { name: string; layout: unknown } =>
        !!p &&
        typeof p === 'object' &&
        typeof (p as Preset).name === 'string' &&
        !!(p as Preset).name.trim(),
    )
    .slice(0, 30)
    .map((p) => ({ name: p.name.trim().slice(0, 40), layout: normaliseLayout(p.layout) }));
}

/// CSS grid columns for the song list: each column at its width, shrinking when the window is
/// too narrow; the title takes what's left.
export function columnTemplate(columns: Column[]): string {
  return columns
    .map((c) =>
      c.id === 'title' ? 'minmax(120px, 1fr)' : `minmax(${Math.min(40, c.width)}px, ${c.width}px)`,
    )
    .join(' ');
}

/// Moves an item one place up (-1) or down (+1).
export function move<T>(list: T[], index: number, by: -1 | 1): T[] {
  const to = index + by;
  if (to < 0 || to >= list.length) return list;
  const out = [...list];
  [out[index], out[to]] = [out[to] as T, out[index] as T];
  return out;
}

/// Moves `from` to where `to` is (drag and drop).
export function moveTo<T>(list: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= list.length || to >= list.length) return list;
  const out = [...list];
  const [item] = out.splice(from, 1);
  out.splice(to, 0, item as T);
  return out;
}
