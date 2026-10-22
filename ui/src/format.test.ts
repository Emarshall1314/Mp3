import { describe, expect, it } from 'vitest';
import { duration, longDuration, qualityLabel, streamSpec, trackSpec } from './format';

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
        resampled: false,
      }),
    ).toBe('PCM · 32-bit float/192 kHz');
  });
});
