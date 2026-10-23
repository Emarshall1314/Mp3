// Typed wrappers over the shell's commands. Every call into the core goes through this module.
import { invoke as call } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  AlbumDetail,
  AlbumRow,
  AlbumSort,
  ApiEvent,
  Appearance,
  Browse,
  SavedTheme,
  ArtistPage,
  ArtistRow,
  EditBatch,
  EditStatus,
  BuildInfo,
  DspSettings,
  EqBand,
  Home,
  Rediscover,
  ImportedEq,
  LibraryStats,
  LoudnessStatus,
  Lyrics,
  Outputs,
  PlayerState,
  Remote,
  Repeat,
  ReplayGainSettings,
  Root,
  SavedSearch,
  ScanStatus,
  SearchResults,
  Settings,
  RenamePlan,
  SongSort,
  StoredArt,
  SyncConflict,
  SyncFolderInfo,
  SyncSettings,
  SyncStatus,
  TrackEdit,
  TrashItem,
  TrackRow,
  TrackTags,
} from './types';

let opened: Promise<void> | null = null;

/// Resolves once the core is open. On Android the page starts while the shell is still opening
/// the library; a call before then fails ("state not managed"), which used to show the default
/// look at every start. Gives up waiting after 30 s, so a core that never opens still reports
/// errors rather than hanging.
export function whenOpen(): Promise<void> {
  opened ??= (async () => {
    for (let i = 0; i < 600; i++) {
      try {
        if (await call<boolean>('ready')) return;
      } catch {
        return;
      }
      await new Promise((r) => setTimeout(r, 50));
    }
  })();
  return opened;
}

let onPlay: ((album?: number) => void) | null = null;

/// Called whenever a new queue starts from a page (an album, a list of songs), so the app can
/// remember where the music came from (swiping right in Now Playing goes back there).
export function setPlayListener(listener: (album?: number) => void) {
  onPlay = listener;
}

function played<T>(call: Promise<T>, album?: number): Promise<T> {
  onPlay?.(album);
  return call;
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  await whenOpen();
  return call<T>(command, args);
}

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
  browse: (genre: string | null, artist: string | null) =>
    invoke<Browse>('browse', { genre, artist }),
  artistPage: (name: string) => invoke<ArtistPage>('artist_page', { name }),
  home: () => invoke<Home>('home'),
  /// Today's Rediscover, by the owner's clock (days start at their midnight).
  rediscover: () =>
    invoke<Rediscover>('rediscover', { offsetMin: -new Date().getTimezoneOffset() }),
  search: (query: string, limit: number) => invoke<SearchResults>('search', { query, limit }),
  savedSearches: () => invoke<SavedSearch[]>('saved_searches'),
  saveSearch: (name: string, query: string) => invoke<SavedSearch>('save_search', { name, query }),
  deleteSavedSearch: (id: number) => invoke<null>('delete_saved_search', { id }),
  playTracks: (ids: number[], start: number, shuffle = false) =>
    played(invoke<null>('play_tracks', { ids, start, shuffle })),
  playAlbum: (albumId: number, start: number, shuffle = false) =>
    played(invoke<null>('play_album', { albumId, start, shuffle }), albumId),
  playSongs: (sort: SongSort, desc: boolean, index: number, shuffle = false) =>
    played(invoke<null>('play_songs', { sort, desc, index, shuffle })),
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
  storageAccess: () => invoke<boolean>('storage_access'),
  requestStorageAccess: () => invoke<boolean>('request_storage_access'),
  pickMusicFolder: () => invoke<string | null>('pick_music_folder'),
  toggleMiniPlayer: () => invoke<null>('toggle_mini_player'),
  trackTags: (ids: number[]) => invoke<TrackTags[]>('track_tags', { ids }),
  editTracks: (edits: TrackEdit[], description: string) =>
    invoke<number>('edit_tracks', { edits, description }),
  editStatus: () => invoke<EditStatus>('edit_status'),
  cancelEdit: () => invoke<null>('cancel_edit'),
  editHistory: (limit: number) => invoke<EditBatch[]>('edit_history', { limit }),
  undoEdit: (batch: number) => invoke<number>('undo_edit', { batch }),
  importArtwork: (path: string) => invoke<StoredArt>('import_artwork', { path }),
  saveCoverToFolder: (trackId: number) => invoke<string>('save_cover_to_folder', { trackId }),
  previewOrganise: (ids: number[], pattern: string) =>
    invoke<RenamePlan[]>('preview_organise', { ids, pattern }),
  organise: (ids: number[], pattern: string) => invoke<number>('organise', { ids, pattern }),
  setTagBackups: (on: boolean) => invoke<null>('set_tag_backups', { on }),
  startupProblems: () => invoke<string[]>('startup_problems'),
  setFavourite: (ids: number[], on: boolean) => invoke<null>('set_favourite', { ids, on }),
  trash: (ids: number[]) => invoke<TrashItem[]>('trash', { ids }),
  trashItems: () => invoke<TrashItem[]>('trash_items'),
  restore: (id: number) => invoke<null>('restore', { id }),
  purge: (id: number) => invoke<null>('purge', { id }),
  emptyTrash: () => invoke<null>('empty_trash'),
  syncStatus: () => invoke<SyncStatus>('sync_status'),
  setSyncSettings: (
    settings: SyncSettings,
    passphrase: string | null = null,
    password: string | null = null,
  ) => invoke<SyncStatus>('set_sync_settings', { settings, passphrase, password }),
  syncRemoteInfo: (remote: Remote, password: string) =>
    invoke<SyncFolderInfo>('sync_remote_info', { remote, password }),
  syncFolderInfo: (folder: string) => invoke<SyncFolderInfo>('sync_folder_info', { folder }),
  syncNow: () => invoke<null>('sync_now'),
  continueFrom: (deviceId: string) => invoke<null>('continue_from', { deviceId }),
  syncConflicts: () => invoke<SyncConflict[]>('sync_conflicts'),
  resolveConflict: (audioHash: string, field: string, values: string[]) =>
    invoke<null>('resolve_conflict', { audioHash, field, values }),
  setAppearance: (appearance: Appearance) => invoke<null>('set_appearance', { appearance }),
  themes: () => invoke<SavedTheme[]>('themes'),
  safeMode: () => invoke<boolean>('safe_mode'),
  setGlobalShortcuts: (bindings: [string, string][]) =>
    invoke<{ wayland: boolean; failed: [string, string][] }>('set_global_shortcuts', { bindings }),
  setVisualizer: (on: boolean) => invoke<null>('set_visualizer', { on }),
  spectrum: (bands: number) => invoke<number[]>('spectrum', { bands }),
  saveTheme: (
    id: string | null,
    theme: SavedTheme['theme'] | Omit<SavedTheme['theme'], 'format' | 'version'>,
  ) => invoke<SavedTheme>('save_theme', { id, theme }),
  importTheme: (text: string) => invoke<SavedTheme>('import_theme', { text }),
  deleteTheme: (id: string) => invoke<null>('delete_theme', { id }),
  exportTheme: (
    theme: SavedTheme['theme'] | Omit<SavedTheme['theme'], 'format' | 'version'>,
    path: string,
  ) => invoke<string>('export_theme', { theme, path }),
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

/// Android has no separate windows: no mini player, no tray.
export const isAndroid = /Android/.test(navigator.userAgent);

/// A system-wide shortcut was pressed: the action's id.
export function onGlobalShortcut(handler: (action: string) => void): Promise<UnlistenFn> {
  return listen<string>('cadenza://global-shortcut', (msg) => handler(msg.payload));
}

/// Saved themes changed (saved, imported or deleted in any window).
export function onThemesChanged(handler: () => void): Promise<UnlistenFn> {
  return listen('cadenza://themes', () => handler());
}

/// Appearance changes, from whichever window made them.
export function onAppearance(handler: (a: Appearance) => void): Promise<UnlistenFn> {
  return listen<Appearance>('cadenza://appearance', (msg) => handler(msg.payload));
}

/// Artwork thumbnail URL served by the shell's `cadenza` scheme. Webviews that cannot load
/// custom schemes directly (Android, Windows) reach them as http://<scheme>.localhost.
/// Thumbnail sizes, in device pixels. Requests snap up to one of these, so the core's cache
/// holds a few sizes per album instead of one for every card width (each new size is made on
/// first request, which shows as a stutter while scrolling).
const ART_SIZES = [64, 128, 192, 256, 384, 512, 768, 1024];

export function artSize(size: number, ratio = globalThis.devicePixelRatio || 1): number {
  const px = Math.ceil(size * ratio);
  return ART_SIZES.find((s) => s >= px) ?? Math.min(px, 2048);
}

export function artUrl(hash: string, size: number): string {
  const px = artSize(size);
  const base = /Android|Windows/.test(navigator.userAgent)
    ? 'http://cadenza.localhost'
    : 'cadenza://localhost';
  return `${base}/art/${hash}?s=${px}`;
}
