// Mirrors of the Rust types the shell sends (serde camelCase). Keep in step with cadenza-api.

export type SongSort =
  'title' | 'artist' | 'album' | 'year' | 'duration' | 'added' | 'genre' | 'plays' | 'rating';
export type AlbumSort = 'artist' | 'title' | 'year' | 'added';
export type Quality = 'lossy' | 'lossless' | 'hires' | 'dsd';

export interface TrackRow {
  id: number;
  uid: string;
  title: string;
  artist: string;
  album: string;
  albumartist: string;
  albumId: number | null;
  durationMs: number;
  trackNo: number | null;
  discNo: number | null;
  year: number | null;
  genre: string;
  codec: string;
  sampleRate: number;
  bitDepth: number | null;
  dsdRate: number | null;
  bitrate: number;
  quality: Quality;
  artHash: string | null;
  rating: number | null;
  favourite: boolean;
  playCount: number;
}

export interface AlbumRow {
  id: number;
  title: string;
  albumartist: string;
  year: number | null;
  genre: string | null;
  trackCount: number;
  durationMs: number;
  quality: Quality | null;
  artHash: string | null;
  compilation: boolean;
  addedAt: number;
}

export interface AlbumDetail {
  album: AlbumRow;
  date: string | null;
  label: string | null;
  discCount: number;
  tracks: TrackRow[];
  otherVersions: AlbumRow[];
}

export interface ArtistRow {
  name: string;
  albumCount: number;
  artHash: string | null;
}

export interface SearchResults {
  songs: TrackRow[];
  songCount: number;
  songCountCapped: boolean;
  albums: AlbumRow[];
  artists: ArtistRow[];
}

export interface SavedSearch {
  id: number;
  name: string;
  query: string;
}

export interface Root {
  id: number;
  path: string;
}

export interface LibraryStats {
  tracks: number;
  albums: number;
  artists: number;
  durationMs: number;
}

export interface ScanProgress {
  phase: 'walking' | 'reading' | 'writing' | 'done';
  found: number;
  toRead: number;
  read: number;
}

export interface ScanReport {
  added: number;
  updated: number;
  moved: number;
  missing: number;
  unchanged: number;
  failed: number;
  errors: [string, string][];
  elapsedMs: number;
}

export interface ScanStatus {
  running: boolean;
  progress: ScanProgress | null;
  lastReport: ScanReport | null;
}

export type Status = 'stopped' | 'playing' | 'paused';
export type Repeat = 'off' | 'all' | 'one';

export interface StreamInfo {
  codec: string;
  lossless: boolean;
  sampleRate: number;
  bits: number | null;
  float: boolean;
  dsdRate: number | null;
  channels: number;
  output: string;
  outputRate: number;
  outputChannels: number;
  outputFormat: DeviceFormat;
  /** The output reaches the device directly, without the OS mixer. */
  exclusive: boolean;
  resampled: boolean;
  /** The device receives exactly the file's samples. */
  bitPerfect: boolean;
}

export type DeviceFormat = { kind: 'float' } | { kind: 'int'; bits: number };

export interface PlayerState {
  status: Status;
  queueLength: number;
  queueVersion: number;
  index: number | null;
  trackId: number | null;
  positionMs: number;
  durationMs: number;
  volume: number;
  repeat: Repeat;
  shuffle: boolean;
  stream: StreamInfo | null;
}

export type PlayerEvent =
  | ({ type: 'state' } & PlayerState)
  | { type: 'position'; trackId: number; positionMs: number }
  | { type: 'trackFinished'; trackId: number; playedMs: number; completed: boolean }
  | { type: 'error'; trackId: number | null; message: string };

export type ApiEvent =
  | { kind: 'player'; event: PlayerEvent }
  | { kind: 'scanProgress'; progress: ScanProgress }
  | { kind: 'scanFinished'; report: ScanReport }
  | { kind: 'libraryChanged' };

export interface BuildInfo {
  name: string;
  version: string;
  targetOs: string;
  targetArch: string;
  debug: boolean;
}

export interface OutputDevice {
  id: string;
  name: string;
  /** Opened exclusively (bit-perfect capable) rather than through the OS mixer. */
  exclusive: boolean;
}

export interface Outputs {
  devices: OutputDevice[];
  current: string;
}
