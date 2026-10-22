import type { AlbumRow, Quality, StreamInfo, TrackRow } from './types';

/// 3:07, 1:02:03
export function duration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/// "1 hour, 12 minutes" / "42 minutes"
export function longDuration(ms: number): string {
  const minutes = Math.round(ms / 60000);
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  const mins = `${m} minute${m === 1 ? '' : 's'}`;
  return h > 0 ? `${h} hour${h === 1 ? '' : 's'}, ${mins}` : mins;
}

export function khz(rate: number): string {
  const k = rate / 1000;
  return `${Number.isInteger(k) ? k : k.toFixed(1)} kHz`;
}

const CODEC_NAMES: Record<string, string> = {
  flac: 'FLAC',
  alac: 'ALAC',
  pcm: 'PCM',
  wavpack: 'WavPack',
  ape: "Monkey's Audio",
  dsd: 'DSD',
  mp3: 'MP3',
  aac: 'AAC',
  opus: 'Opus',
  vorbis: 'Vorbis',
};

export function codecName(codec: string): string {
  return CODEC_NAMES[codec] ?? codec.toUpperCase();
}

export function qualityLabel(q: Quality | null | undefined): string | null {
  switch (q) {
    case 'dsd':
      return 'DSD';
    case 'hires':
      return 'Hi-Res Lossless';
    case 'lossless':
      return 'Lossless';
    default:
      return null;
  }
}

/// "FLAC · 24-bit/96 kHz · 2,950 kbps", "DSD64 · 2.8 MHz", "AAC · 256 kbps"
export function trackSpec(
  t: Pick<TrackRow, 'codec' | 'sampleRate' | 'bitDepth' | 'dsdRate' | 'bitrate' | 'quality'>,
): string {
  if (t.dsdRate) {
    return `DSD${Math.round(t.dsdRate / 44100)} · ${(t.dsdRate / 1e6).toFixed(1)} MHz`;
  }
  const parts = [codecName(t.codec)];
  if (t.quality !== 'lossy' && t.bitDepth) parts.push(`${t.bitDepth}-bit/${khz(t.sampleRate)}`);
  else parts.push(khz(t.sampleRate));
  if (t.bitrate > 0) parts.push(`${t.bitrate.toLocaleString('en-US')} kbps`);
  return parts.join(' · ');
}

/// What is playing, as delivered: "FLAC · 24-bit/96 kHz", "DSD64 → PCM 176.4 kHz".
export function streamSpec(s: StreamInfo): string {
  if (s.dsdRate) return `DSD${Math.round(s.dsdRate / 44100)} · converted to PCM`;
  const depth = s.float ? '32-bit float' : s.bits ? `${s.bits}-bit` : null;
  return [codecName(s.codec), depth ? `${depth}/${khz(s.sampleRate)}` : khz(s.sampleRate)].join(
    ' · ',
  );
}

export function albumMeta(a: AlbumRow): string {
  return [a.genre, a.year].filter(Boolean).join(' · ');
}

export function trackNumber(t: TrackRow): string {
  return t.trackNo ? String(t.trackNo) : '';
}
