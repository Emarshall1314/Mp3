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
  /** DSP stages running on this stream, in order. */
  dsp: string[];
  /** ReplayGain applied to this track, in dB. */
  replayGainDb: number | null;
}

export type DeviceFormat =
  | { kind: 'float' }
  | { kind: 'int'; bits: number }
  /** DSD over PCM in a container with this many significant bits. */
  | { kind: 'dop'; bits: number }
  /** Native DSD: bytes per word. */
  | { kind: 'dsd'; bytes: number; bigEndian: boolean };

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
  | { kind: 'libraryChanged' }
  | { kind: 'loudness'; status: LoudnessStatus };

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
  /** Devices with DSD over DoP turned on. */
  dop: string[];
}

export type FilterKind = 'peak' | 'lowShelf' | 'highShelf' | 'lowPass' | 'highPass';

export interface EqBand {
  kind: FilterKind;
  freq: number;
  gainDb: number;
  q: number;
}

export type CrossfeedLevel = 'default' | 'chuMoy' | 'meier';

export interface DspSettings {
  preampDb: number;
  eqEnabled: boolean;
  eq: EqBand[];
  crossfeedEnabled: boolean;
  crossfeed: CrossfeedLevel;
  convolutionEnabled: boolean;
  convolutionPath: string | null;
  limiter: boolean;
}

export type ReplayGainMode = 'off' | 'track' | 'album' | 'auto';

export interface ReplayGainSettings {
  mode: ReplayGainMode;
  preampDb: number;
  untaggedDb: number;
  preventClipping: boolean;
}

export interface Settings {
  outputDevice: string;
  dopDevices: string[];
  dsp: Record<string, DspSettings>;
  replayGain: ReplayGainSettings;
  crossfadeS: number;
}

export interface LoudnessStatus {
  running: boolean;
  done: number;
  total: number;
}

export interface ImportedEq {
  preampDb: number;
  bands: EqBand[];
}

export interface LyricLine {
  /** Start in ms; null when the lyrics are not synced. */
  timeMs: number | null;
  text: string;
}

export interface Lyrics {
  synced: boolean;
  lines: LyricLine[];
  source: 'file' | 'embedded';
}
