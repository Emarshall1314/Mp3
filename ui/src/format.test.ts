import { describe, expect, it } from 'vitest';
import { duration, longDuration, qualityLabel, signalPath, streamSpec, trackSpec } from './format';

describe('format', () => {
  it('formats durations', () => {
    expect(duration(0)).toBe('0:00');
    expect(duration(187_000)).toBe('3:07');
    expect(duration(3_723_000)).toBe('1:02:03');
    expect(longDuration(4_320_000)).toBe('1 hour, 12 minutes');
    expect(longDuration(60_000)).toBe('1 minute');
  });

  it('describes tracks like the spec example', () => {
    expect(
      trackSpec({
        codec: 'flac',
        sampleRate: 96000,
        bitDepth: 24,
        dsdRate: null,
        bitrate: 2950,
        quality: 'hires',
      }),
    ).toBe('FLAC · 24-bit/96 kHz · 2,950 kbps');
    expect(
      trackSpec({
        codec: 'aac',
        sampleRate: 44100,
        bitDepth: null,
        dsdRate: null,
        bitrate: 256,
        quality: 'lossy',
      }),
    ).toBe('AAC · 44.1 kHz · 256 kbps');
    expect(
      trackSpec({
        codec: 'dsd',
        sampleRate: 2822400,
        bitDepth: 1,
        dsdRate: 2822400,
        bitrate: 5644,
        quality: 'dsd',
      }),
    ).toBe('DSD64 · 2.8 MHz');
  });

  it('labels quality', () => {
    expect(qualityLabel('hires')).toBe('Hi-Res Lossless');
    expect(qualityLabel('lossy')).toBeNull();
  });

  it('describes the playing stream', () => {
    expect(
      streamSpec({
        codec: 'pcm',
        lossless: true,
        sampleRate: 192000,
        bits: null,
        float: true,
        dsdRate: null,
        channels: 2,
        output: '',
        outputRate: 192000,
        outputChannels: 2,
        outputFormat: { kind: 'float' },
        exclusive: false,
        bitPerfect: false,
        dsp: [],
        replayGainDb: null,
        resampled: false,
      }),
    ).toBe('PCM · 32-bit float/192 kHz');
  });

  it('explains the signal path', () => {
    const base = {
      codec: 'flac',
      lossless: true,
      sampleRate: 96000,
      bits: 24,
      float: false,
      dsdRate: null,
      channels: 2,
      output: '',
      outputRate: 96000,
      outputChannels: 2,
      outputFormat: { kind: 'int', bits: 24 } as const,
      exclusive: true,
      resampled: false,
      bitPerfect: true,
      dsp: [],
      replayGainDb: null,
    };
    const perfect = signalPath(base, 1);
    expect(perfect.bitPerfect).toBe(true);
    expect(perfect.reasons).toEqual([]);
    expect(perfect.steps.map((s) => s.label)).toEqual(['Source', 'Output']);

    const shared = signalPath(
      {
        ...base,
        exclusive: false,
        bitPerfect: false,
        resampled: true,
        outputRate: 48000,
        outputFormat: { kind: 'float' },
        dsp: ['EQ · 3 bands'],
        replayGainDb: -6.5,
      },
      0.5,
    );
    expect(shared.steps.map((s) => s.label)).toEqual([
      'Source',
      'ReplayGain',
      'Resampled',
      'DSP',
      'Volume',
      'Output',
    ]);
    expect(shared.reasons).toHaveLength(5);

    const dop = signalPath(
      {
        ...base,
        codec: 'dsf',
        bits: null,
        dsdRate: 2822400,
        outputFormat: { kind: 'dop', bits: 24 },
      },
      1,
    );
    expect(dop.steps[0]?.detail).toBe('DSD64 · as DoP · 2 ch');
  });
});
