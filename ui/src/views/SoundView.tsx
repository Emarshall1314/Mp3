import { useEffect, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type {
  CrossfeedLevel,
  DspSettings,
  Outputs,
  ReplayGainMode,
  ReplayGainSettings,
  Settings,
} from '../types';
import { EqEditor } from '../components/EqEditor';
import { Range } from '../components/Range';
import { Select } from '../components/Select';
import { SegmentLens } from '../components/SegmentLens';

const DEFAULT_DSP: DspSettings = {
  preampDb: 0,
  eqEnabled: false,
  eq: [],
  crossfeedEnabled: false,
  crossfeed: 'default',
  convolutionEnabled: false,
  convolutionPath: null,
  limiter: false,
};

const RG_MODES: { mode: ReplayGainMode; label: string }[] = [
  { mode: 'off', label: 'Off' },
  { mode: 'track', label: 'Track' },
  { mode: 'album', label: 'Album' },
  { mode: 'auto', label: 'Auto' },
];

const CROSSFEED: { level: CrossfeedLevel; label: string }[] = [
  { level: 'default', label: 'Subtle (700 Hz, 4.5 dB)' },
  { level: 'chuMoy', label: 'Chu Moy (700 Hz, 6 dB)' },
  { level: 'meier', label: 'Jan Meier (650 Hz, 9.5 dB)' },
];

/// Runs `save` once changes stop for a moment, so dragging a slider doesn't flood the core.
function useDebounced<T>(save: (v: T) => Promise<unknown>) {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latest = useRef<T | null>(null);
  useEffect(
    () => () => {
      if (timer.current && latest.current !== null) {
        clearTimeout(timer.current);
        void attempt(save(latest.current));
      }
    },
    [save],
  );
  return (v: T) => {
    latest.current = v;
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      timer.current = null;
      latest.current = null;
      void attempt(save(v));
    }, 120);
  };
}

/// Sound: ReplayGain, crossfade and the current output's DSP profile.
export function SoundView() {
  const settingsVersion = useStore((s) => s.settingsVersion);
  const loudness = useStore((s) => s.loudness);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [outputs, setOutputs] = useState<Outputs | null>(null);

  useEffect(() => {
    void api.settings().then(setSettings);
    void api.outputs().then(setOutputs);
  }, [settingsVersion]);

  const saveDsp = useDebounced(api.setDsp);
  const saveRg = useDebounced(api.setReplayGain);
  const saveCrossfade = useDebounced(api.setCrossfade);

  if (!settings) return <section className="view" aria-busy="true" />;
  const device = outputs?.devices.find((d) => d.id === settings.outputDevice);
  const dsp = settings.dsp[settings.outputDevice] ?? DEFAULT_DSP;
  const rg = settings.replayGain;

  const setDsp = (patch: Partial<DspSettings>) => {
    const next = { ...dsp, ...patch };
    setSettings({ ...settings, dsp: { ...settings.dsp, [settings.outputDevice]: next } });
    saveDsp(next);
  };
  const setRg = (patch: Partial<ReplayGainSettings>) => {
    const next = { ...rg, ...patch };
    setSettings({ ...settings, replayGain: next });
    saveRg(next);
  };

  const chooseIr = async () => {
    const path = await open({
      title: 'Choose Impulse Response',
      filters: [{ name: 'Audio', extensions: ['wav', 'flac', 'aif', 'aiff', 'wv'] }],
    });
    if (typeof path === 'string') setDsp({ convolutionPath: path, convolutionEnabled: true });
  };

  return (
    <section className="view scroll sound-view">
      <header className="view-header">
        <h1>Sound</h1>
      </header>

      <div className="settings-group">
        <h2>ReplayGain</h2>
        <p className="muted">
          Evens out loudness between tracks using ReplayGain tags, or Cadenza’s own EBU R128
          measurement for files without them. Files are never changed.
        </p>
        <div className="segmented" role="radiogroup" aria-label="ReplayGain mode">
          <SegmentLens />
          {RG_MODES.map(({ mode, label }) => (
            <button
              key={mode}
              role="radio"
              aria-checked={rg.mode === mode}
              className={rg.mode === mode ? 'on' : ''}
              onClick={() => setRg({ mode })}
            >
              {label}
            </button>
          ))}
        </div>
        <p className="muted small">
          Auto uses album gain while an album plays in order, track gain otherwise.
        </p>
        <label className="slider-row">
          <span>Preamp</span>
          <Range
            min={-12}
            max={12}
            step={0.5}
            value={rg.preampDb}
            disabled={rg.mode === 'off'}
            onChange={(e) => setRg({ preampDb: Number(e.target.value) })}
          />
          <output>{rg.preampDb.toFixed(1)} dB</output>
        </label>
        <label className="slider-row">
          <span>Without ReplayGain</span>
          <Range
            min={-12}
            max={0}
            step={0.5}
            value={rg.untaggedDb}
            disabled={rg.mode === 'off'}
            onChange={(e) => setRg({ untaggedDb: Number(e.target.value) })}
          />
          <output>{rg.untaggedDb.toFixed(1)} dB</output>
        </label>
        <label className="check-row">
          <input
            type="checkbox"
            checked={rg.preventClipping}
            disabled={rg.mode === 'off'}
            onChange={(e) => setRg({ preventClipping: e.target.checked })}
          />
          Prevent clipping (lower the gain where a track’s peak would exceed full scale)
        </label>
        <div className="loudness-status">
          {loudness?.running ? (
            <p role="status">
              Measuring loudness: {loudness.done.toLocaleString()} of{' '}
              {loudness.total.toLocaleString()} tracks
            </p>
          ) : (
            <button className="pill" onClick={() => void attempt(api.analyzeLoudness())}>
              Measure Tracks Without ReplayGain
            </button>
          )}
        </div>
      </div>

      <div className="settings-group">
        <h2>Crossfade</h2>
        <label className="slider-row">
          <span>Length</span>
          <Range
            min={0}
            max={12}
            step={0.5}
            value={settings.crossfadeS}
            onChange={(e) => {
              const seconds = Number(e.target.value);
              setSettings({ ...settings, crossfadeS: seconds });
              saveCrossfade(seconds);
            }}
          />
          <output>{settings.crossfadeS === 0 ? 'Off' : `${settings.crossfadeS} s`}</output>
        </label>
        <p className="muted small">
          Tracks from the same album stay gapless. Tracks with a different sample rate or channel
          count change over without a fade.
        </p>
      </div>

      <div className="dsp-heading">
        <h2>Processing for {device?.name ?? settings.outputDevice}</h2>
        <p className="muted">
          Each output keeps its own settings. While any of these is on, playback is not bit-perfect.
        </p>
      </div>

      <div className="settings-group">
        <label className="check-row heading">
          <input
            type="checkbox"
            checked={dsp.eqEnabled}
            onChange={(e) => setDsp({ eqEnabled: e.target.checked })}
          />
          <h2>Equalizer</h2>
        </label>
        <EqEditor dsp={dsp} onChange={(d) => setDsp(d)} />
      </div>

      <div className="settings-group">
        <label className="check-row heading">
          <input
            type="checkbox"
            checked={dsp.crossfeedEnabled}
            onChange={(e) => setDsp({ crossfeedEnabled: e.target.checked })}
          />
          <h2>Crossfeed</h2>
        </label>
        <p className="muted small">
          For headphones: blends a little of each channel into the other, as speakers would, so
          hard-panned recordings tire the ears less. Stereo outputs only.
        </p>
        <Select<CrossfeedLevel>
          label="Crossfeed strength"
          value={dsp.crossfeed}
          options={CROSSFEED.map((c) => ({ value: c.level, label: c.label }))}
          onChange={(crossfeed) => setDsp({ crossfeed })}
        />
      </div>

      <div className="settings-group">
        <label className="check-row heading">
          <input
            type="checkbox"
            checked={dsp.convolutionEnabled}
            disabled={!dsp.convolutionPath}
            onChange={(e) => setDsp({ convolutionEnabled: e.target.checked })}
          />
          <h2>Convolution</h2>
        </label>
        <p className="muted small">
          Applies an impulse response (room correction, headphone target): a WAV or FLAC file, mono
          or one channel per output channel, up to 10 seconds. Adds 1024 samples of latency.
        </p>
        <div className="file-row">
          <span className="path">{dsp.convolutionPath ?? 'No file chosen'}</span>
          <button className="pill" onClick={() => void chooseIr()}>
            Choose…
          </button>
          {dsp.convolutionPath && (
            <button
              className="pill"
              onClick={() => setDsp({ convolutionPath: null, convolutionEnabled: false })}
            >
              Remove
            </button>
          )}
        </div>
      </div>

      <div className="settings-group">
        <label className="check-row heading">
          <input
            type="checkbox"
            checked={dsp.limiter}
            onChange={(e) => setDsp({ limiter: e.target.checked })}
          />
          <h2>Limiter</h2>
        </label>
        <p className="muted small">
          Catches peaks that EQ boosts or ReplayGain push past full scale, so they never clip
          (ceiling −0.3 dBFS, 1.5 ms lookahead).
        </p>
      </div>
    </section>
  );
}
