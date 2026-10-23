import { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { FileUp, Plus, RotateCcw, Trash2 } from 'lucide-react';
import { api } from '../api';
import { attempt } from '../store';
import type { DspSettings, EqBand, FilterKind } from '../types';
import { Range } from './Range';
import { Select } from './Select';

const GRAPHIC_FREQS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
const GRAPHIC_Q = 1.41;

const PRESETS: { name: string; gains: number[] }[] = [
  { name: 'Flat', gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  { name: 'Bass Boost', gains: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0] },
  { name: 'Bass Reducer', gains: [-6, -5, -4, -2, 0, 0, 0, 0, 0, 0] },
  { name: 'Treble Boost', gains: [0, 0, 0, 0, 0, 0, 2, 4, 5, 6] },
  { name: 'Vocal', gains: [-2, -2, -1, 1, 3, 3, 2, 1, 0, -1] },
  { name: 'Loudness', gains: [5, 4, 2, 0, -1, 0, 0, 1, 3, 4] },
];

const KINDS: { kind: FilterKind; label: string }[] = [
  { kind: 'peak', label: 'Peak' },
  { kind: 'lowShelf', label: 'Low shelf' },
  { kind: 'highShelf', label: 'High shelf' },
  { kind: 'lowPass', label: 'Low pass' },
  { kind: 'highPass', label: 'High pass' },
];

const graphicBands = (gains: number[]): EqBand[] =>
  GRAPHIC_FREQS.map((freq, i) => ({ kind: 'peak', freq, gainDb: gains[i] ?? 0, q: GRAPHIC_Q }));

const isGraphic = (bands: EqBand[]) =>
  bands.length === GRAPHIC_FREQS.length &&
  bands.every((b, i) => b.kind === 'peak' && b.freq === GRAPHIC_FREQS[i] && b.q === GRAPHIC_Q);

const freqLabel = (f: number) => (f >= 1000 ? `${f / 1000}k` : `${f}`);

/// Graphic (10 bands) or parametric EQ with its response curve. Changes go to `onChange` at once.
export function EqEditor({
  dsp,
  onChange,
}: {
  dsp: DspSettings;
  onChange: (d: DspSettings) => void;
}) {
  const graphic = dsp.eq.length === 0 || isGraphic(dsp.eq);
  const [parametric, setParametric] = useState(!graphic);
  const bands = dsp.eq.length === 0 ? graphicBands([]) : dsp.eq;
  const set = (patch: Partial<DspSettings>) => onChange({ ...dsp, ...patch });
  const setBand = (i: number, patch: Partial<EqBand>) =>
    set({ eq: bands.map((b, j) => (j === i ? { ...b, ...patch } : b)) });

  const importAutoeq = async () => {
    const path = await open({
      title: 'Import AutoEq ParametricEQ.txt',
      filters: [{ name: 'AutoEq parametric EQ', extensions: ['txt'] }],
    });
    if (typeof path !== 'string') return;
    const imported = await attempt(api.importAutoeq(path));
    if (imported) {
      setParametric(true);
      set({ eqEnabled: true, preampDb: imported.preampDb, eq: imported.bands });
    }
  };

  return (
    <div className="eq-editor">
      <EqCurve preampDb={dsp.preampDb} bands={dsp.eqEnabled ? bands : []} />
      <div className="eq-toolbar">
        <div className="segmented" role="radiogroup" aria-label="EQ type">
          <button
            role="radio"
            aria-checked={!parametric}
            className={!parametric ? 'on' : ''}
            onClick={() => {
              if (!graphic) set({ eq: graphicBands([]) });
              setParametric(false);
            }}
            title={graphic ? undefined : 'Replaces the parametric bands with a flat graphic EQ'}
          >
            Graphic
          </button>
          <button
            role="radio"
            aria-checked={parametric}
            className={parametric ? 'on' : ''}
            onClick={() => setParametric(true)}
          >
            Parametric
          </button>
        </div>
        {!parametric && (
          <Select
            label="Preset"
            value=""
            placeholder="Preset…"
            options={PRESETS.map((p) => ({ value: p.name, label: p.name }))}
            onChange={(name) => {
              const p = PRESETS.find((p) => p.name === name);
              if (p) set({ eq: graphicBands(p.gains), eqEnabled: true });
            }}
          />
        )}
        <button className="pill" onClick={() => void importAutoeq()}>
          <FileUp size={14} aria-hidden /> Import AutoEq…
        </button>
        <button
          className="pill"
          onClick={() => set({ eq: graphicBands([]), preampDb: 0 })}
          title="Flat EQ, preamp 0 dB"
        >
          <RotateCcw size={14} aria-hidden /> Reset
        </button>
      </div>

      <label className="slider-row">
        <span>Preamp</span>
        <Range
          min={-24}
          max={12}
          step={0.5}
          value={dsp.preampDb}
          onChange={(e) => set({ preampDb: Number(e.target.value) })}
        />
        <output>{dsp.preampDb.toFixed(1)} dB</output>
      </label>

      {!parametric ? (
        <div className="graphic-eq" role="group" aria-label="Graphic EQ">
          {bands.map((b, i) => (
            <label key={b.freq} className="graphic-band">
              <output>
                {b.gainDb > 0 ? '+' : ''}
                {b.gainDb}
              </output>
              <span className="vertical-slot">
                <Range
                  aria-label={`${freqLabel(b.freq)} Hz`}
                  min={-12}
                  max={12}
                  step={0.5}
                  value={b.gainDb}
                  onChange={(e) => setBand(i, { gainDb: Number(e.target.value) })}
                />
              </span>
              <span>{freqLabel(b.freq)}</span>
            </label>
          ))}
        </div>
      ) : (
        <table className="peq">
          <thead>
            <tr>
              <th>Type</th>
              <th>Frequency (Hz)</th>
              <th>Gain (dB)</th>
              <th>Q</th>
              <th aria-label="Remove" />
            </tr>
          </thead>
          <tbody>
            {bands.map((b, i) => (
              <tr key={i}>
                <td>
                  <Select<FilterKind>
                    label={`Band ${i + 1} type`}
                    value={b.kind}
                    options={KINDS.map((k) => ({ value: k.kind, label: k.label }))}
                    onChange={(kind) => setBand(i, { kind })}
                  />
                </td>
                <td>
                  <NumberInput
                    label={`Band ${i + 1} frequency`}
                    value={b.freq}
                    min={10}
                    max={40000}
                    onCommit={(freq) => setBand(i, { freq })}
                  />
                </td>
                <td>
                  <NumberInput
                    label={`Band ${i + 1} gain`}
                    value={b.gainDb}
                    min={-30}
                    max={30}
                    disabled={b.kind === 'lowPass' || b.kind === 'highPass'}
                    onCommit={(gainDb) => setBand(i, { gainDb })}
                  />
                </td>
                <td>
                  <NumberInput
                    label={`Band ${i + 1} Q`}
                    value={b.q}
                    min={0.05}
                    max={30}
                    onCommit={(q) => setBand(i, { q })}
                  />
                </td>
                <td>
                  <button
                    className="icon-button small"
                    aria-label={`Remove band ${i + 1}`}
                    onClick={() => set({ eq: bands.filter((_, j) => j !== i) })}
                  >
                    <Trash2 size={14} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <td colSpan={5}>
                <button
                  className="pill"
                  onClick={() =>
                    set({ eq: [...bands, { kind: 'peak', freq: 1000, gainDb: 0, q: 1 }] })
                  }
                >
                  <Plus size={14} aria-hidden /> Add Band
                </button>
              </td>
            </tr>
          </tfoot>
        </table>
      )}
    </div>
  );
}

/// A number field that applies on Enter or when it loses focus (not on every keystroke).
function NumberInput({
  label,
  value,
  min,
  max,
  disabled,
  onCommit,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  disabled?: boolean;
  onCommit: (v: number) => void;
}) {
  const [text, setText] = useState<string | null>(null);
  const commit = () => {
    if (text === null) return;
    const v = Number(text);
    if (Number.isFinite(v)) onCommit(Math.min(max, Math.max(min, v)));
    setText(null);
  };
  return (
    <input
      type="number"
      aria-label={label}
      value={text ?? String(value)}
      min={min}
      max={max}
      step="any"
      disabled={disabled}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === 'Enter' && commit()}
    />
  );
}

const CURVE_FREQS = Array.from({ length: 97 }, (_, i) => 20 * Math.pow(1000, i / 96));
const W = 600;
const H = 150;
const RANGE = 15;

/// The EQ's frequency response, computed by the core with the real filters.
function EqCurve({ preampDb, bands }: { preampDb: number; bands: EqBand[] }) {
  const [curve, setCurve] = useState<number[]>([]);
  useEffect(() => {
    let live = true;
    void api.eqCurve(preampDb, bands, CURVE_FREQS).then((c) => live && setCurve(c));
    return () => {
      live = false;
    };
  }, [preampDb, bands]);
  const x = (i: number) => (i / (CURVE_FREQS.length - 1)) * W;
  const y = (db: number) => H / 2 - (Math.max(-RANGE, Math.min(RANGE, db)) / RANGE) * (H / 2);
  const clipped = curve.some((db) => db > RANGE || db < -RANGE);
  return (
    <figure className="eq-curve">
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label="EQ frequency response">
        {[-12, -6, 0, 6, 12].map((db) => (
          <line key={db} x1={0} x2={W} y1={y(db)} y2={y(db)} className={db ? 'grid' : 'zero'} />
        ))}
        {[100, 1000, 10000].map((f) => {
          const gx = (Math.log(f / 20) / Math.log(1000)) * W;
          return (
            <g key={f}>
              <line x1={gx} x2={gx} y1={0} y2={H} className="grid" />
              <text x={gx + 4} y={H - 4}>
                {freqLabel(f)}
              </text>
            </g>
          );
        })}
        {curve.length > 0 && (
          <polyline points={curve.map((db, i) => `${x(i)},${y(db)}`).join(' ')} />
        )}
      </svg>
      {clipped && <figcaption className="muted small">The curve goes beyond ±15 dB.</figcaption>}
    </figure>
  );
}
