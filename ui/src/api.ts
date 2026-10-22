// Typed wrappers over the shell's commands. Every call into the core goes through this module.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AlbumDetail,
  AlbumRow,
  AlbumSort,
  ApiEvent,
  ArtistPage,
  ArtistRow,
  BuildInfo,
  DspSettings,
  EqBand,
  Home,
  ImportedEq,
  LibraryStats,
  LoudnessStatus,
  Lyrics,
  Outputs,
  PlayerState,
  Repeat,
  ReplayGainSettings,
  Root,
  SavedSearch,
  ScanStatus,
  SearchResults,
  Settings,
  SongSort,
  TrackRow,
} from './types';

export const api = {
  buildInfo: () => invoke<BuildInfo>('build_info'),
  roots: () => invoke<Root[]>('roots'),
  addRoot: (path: string) => invoke<Root>('add_root', { path }),
  removeRoot: (id: number) => invoke<null>('remove_root', { id }),
  rescan: () => invoke<null>('rescan'),
  scanStatus: () => invoke<ScanStatus>('scan_status'),
  stats: () => invoke<LibraryStats>('stats'),
  songs: (sort: SongSort, desc: boolean, offset: number, limit: number) =>
    invoke<TrackRow[]>('songs', { sort, desc, offset, limit }),
  albums: (sort: AlbumSort, offset: number, limit: number) =>
    invoke<AlbumRow[]>('albums', { sort, offset, limit }),
  album: (id: number) => invoke<AlbumDetail>('album', { id }),
  artists: () => invoke<ArtistRow[]>('artists'),
  artistAlbums: (name: string) => invoke<AlbumRow[]>('artist_albums', { name }),
  artistPage: (name: string) => invoke<ArtistPage>('artist_page', { name }),
  home: () => invoke<Home>('home'),
  search: (query: string, limit: number) => invoke<SearchResults>('search', { query, limit }),
  savedSearches: () => invoke<SavedSearch[]>('saved_searches'),
  saveSearch: (name: string, query: string) => invoke<SavedSearch>('save_search', { name, query }),
  deleteSavedSearch: (id: number) => invoke<null>('delete_saved_search', { id }),
  playTracks: (ids: number[], start: number, shuffle = false) =>
    invoke<null>('play_tracks', { ids, start, shuffle }),
  playAlbum: (albumId: number, start: number, shuffle = false) =>
    invoke<null>('play_album', { albumId, start, shuffle }),
  playSongs: (sort: SongSort, desc: boolean, index: number, shuffle = false) =>
    invoke<null>('play_songs', { sort, desc, index, shuffle }),
  playNext: (ids: number[]) => invoke<null>('play_next', { ids }),
  playLater: (ids: number[]) => invoke<null>('play_later', { ids }),
  player: () => invoke<PlayerState>('player'),
  queueWindow: (offset: number, limit: number) =>
    invoke<TrackRow[]>('queue_window', { offset, limit }),
  tracks: (ids: number[]) => invoke<TrackRow[]>('tracks', { ids }),
  toggle: () => invoke<null>('toggle'),
  next: () => invoke<null>('next'),
  previous: () => invoke<null>('previous'),
  seek: (positionMs: number) => invoke<null>('seek', { positionMs: Math.round(positionMs) }),
  setVolume: (volume: number) => invoke<null>('set_volume', { volume }),
  outputs: () => invoke<Outputs>('outputs'),
  setOutput: (id: string) => invoke<null>('set_output', { id }),
  setDop: (id: string, on: boolean) => invoke<null>('set_dop', { id, on }),
  settings: () => invoke<Settings>('settings'),
  lyrics: (trackId: number) => invoke<Lyrics | null>('lyrics', { trackId }),
  artworkPalette: (hash: string) => invoke<[number, number, number][]>('artwork_palette', { hash }),
  dsp: () => invoke<DspSettings>('dsp'),
  setDsp: (settings: DspSettings) => invoke<null>('set_dsp', { settings }),
  replayGain: () => invoke<ReplayGainSettings>('replay_gain'),
  setReplayGain: (settings: ReplayGainSettings) => invoke<null>('set_replay_gain', { settings }),
  setCrossfade: (seconds: number) => invoke<null>('set_crossfade', { seconds }),
  loudnessStatus: () => invoke<LoudnessStatus>('loudness_status'),
  analyzeLoudness: () => invoke<null>('analyze_loudness'),
  importAutoeq: (path: string) => invoke<ImportedEq>('import_autoeq', { path }),
  eqCurve: (preampDb: number, bands: EqBand[], freqs: number[]) =>
    invoke<number[]>('eq_curve', { preampDb, bands, freqs }),
  playIndex: (index: number) => invoke<null>('play_index', { index }),
  removeFromQueue: (index: number) => invoke<null>('remove_from_queue', { index }),
  moveInQueue: (from: number, to: number) => invoke<null>('move_in_queue', { from, to }),
  clearUpcoming: () => invoke<null>('clear_upcoming'),
  setRepeat: (repeat: Repeat) => invoke<null>('set_repeat', { repeat }),
  setShuffle: (on: boolean) => invoke<null>('set_shuffle', { on }),
};

export function onApiEvent(handler: (e: ApiEvent) => void): Promise<UnlistenFn> {
  return listen<ApiEvent>('cadenza://event', (msg) => handler(msg.payload));
}

/// Artwork thumbnail URL served by the shell's `cadenza` scheme. Webviews that cannot load
/// custom schemes directly (Android, Windows) reach them as http://<scheme>.localhost.
export function artUrl(hash: string, size: number): string {
  const px = Math.ceil(size * (globalThis.devicePixelRatio || 1));
  const base = /Android|Windows/.test(navigator.userAgent)
    ? 'http://cadenza.localhost'
    : 'cadenza://localhost';
  return `${base}/art/${hash}?s=${px}`;
}
