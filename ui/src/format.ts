import type { DeviceFormat, AlbumRow, Quality, StreamInfo, TrackRow } from './types';

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
  if (s.dsdRate) {
    const how =
      s.outputFormat.kind === 'dop'
        ? 'as DoP'
        : s.outputFormat.kind === 'dsd'
          ? 'native'
          : 'converted to PCM';
    return `DSD${Math.round(s.dsdRate / 44100)} · ${how}`;
  }
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

function formatName(f: DeviceFormat): string {
  switch (f.kind) {
    case 'float':
      return '32-bit float';
    case 'int':
      return `${f.bits}-bit integer`;
    case 'dop':
      return `DoP in ${f.bits}-bit`;
    case 'dsd':
      return `native DSD (${f.bytes * 8}-bit words)`;
  }
}

export interface SignalPath {
  /// Each step from file to device, in order.
  steps: { label: string; detail: string }[];
  bitPerfect: boolean;
  /// Why the device does not get the file's exact samples (empty when bit-perfect).
  reasons: string[];
}

/// What happens to the playing stream between the file and the device, and why it is or isn't
/// bit-perfect. The engine decides `bitPerfect`; the reasons explain its decision.
export function signalPath(s: StreamInfo, volume: number): SignalPath {
  const steps: SignalPath['steps'] = [
    { label: 'Source', detail: `${streamSpec(s)} · ${s.channels} ch` },
  ];
  const reasons: string[] = [];
  if (!s.lossless) reasons.push('The source is lossy.');
  if (s.dsdRate && s.outputFormat.kind !== 'dop' && s.outputFormat.kind !== 'dsd') {
    steps.push({ label: 'DSD to PCM', detail: khz(s.outputRate) });
    reasons.push('DSD is converted to PCM: this output takes no DSD (enable DoP if the DAC can).');
  }
  if (s.replayGainDb !== null) {
    steps.push({
      label: 'ReplayGain',
      detail: `${s.replayGainDb > 0 ? '+' : ''}${s.replayGainDb.toFixed(2)} dB`,
    });
    if (s.replayGainDb !== 0) reasons.push('ReplayGain changes the level.');
  }
  if (s.resampled) {
    steps.push({ label: 'Resampled', detail: `${khz(s.sampleRate)} → ${khz(s.outputRate)}` });
    reasons.push('The output does not run at the source’s rate.');
  }
  if (s.outputChannels !== s.channels) {
    steps.push({ label: 'Channels', detail: `${s.channels} → ${s.outputChannels}` });
    reasons.push('Channels are remixed for the output.');
  }
  for (const d of s.dsp) steps.push({ label: 'DSP', detail: d });
  if (s.dsp.length > 0) reasons.push('Sound processing is on (Sound settings).');
  if (volume < 1 && !s.dsdRate) {
    steps.push({ label: 'Volume', detail: `${Math.round(volume * 100)}%` });
    reasons.push('Volume is below 100%.');
  }
  steps.push({
    label: 'Output',
    detail: `${s.exclusive ? 'Exclusive' : 'Shared'} · ${formatName(s.outputFormat)} · ${khz(s.outputRate)}`,
  });
  if (!s.exclusive) reasons.push('Shared output: the system mixer can change the samples.');
  if (s.exclusive && s.bits && s.outputFormat.kind === 'int' && s.outputFormat.bits < s.bits)
    reasons.push(`The device takes ${s.outputFormat.bits} bits; the source has ${s.bits}.`);
  return { steps, bitPerfect: s.bitPerfect, reasons: s.bitPerfect ? [] : reasons };
}
