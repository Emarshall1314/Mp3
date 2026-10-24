import { useRef, useState } from 'react';
import { save as saveDialog } from '@tauri-apps/plugin-dialog';
import { Copy, Download, Pencil, Plus, Trash2, Upload } from 'lucide-react';
import { api, isAndroid } from '../api';
import { changeAppearance, reloadThemes, useLook } from '../hooks/useAppearance';
import { attempt, useStore } from '../store';
import {
  ACCENT_PRESETS,
  BUILT_IN_THEMES,
  DEFAULT_LOOK,
  defaultTheme,
  type Look,
  type Scheme,
  type ThemeDef,
  normaliseLook,
} from '../theme/look';
import type { Appearance, Theme } from '../types';
import { ThemeEditor } from './ThemeEditor';
import { Select } from './Select';
import { SegmentLens } from './SegmentLens';

const MODES: { theme: Theme; label: string }[] = [
  { theme: 'auto', label: 'Auto' },
  { theme: 'light', label: 'Light' },
  { theme: 'dark', label: 'Dark' },
];

function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      <SegmentLens />
      {options.map((o) => (
        <button
          key={o.value}
          role="radio"
          aria-checked={value === o.value}
          className={value === o.value ? 'on' : ''}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function Slider({
  label,
  value,
  min,
  max,
  step = 1,
  unit,
  disabled,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit: string;
  disabled?: boolean;
  onChange: (v: number) => void;
}) {
  return (
    <label className="slider-row">
      <span>{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
      />
      <output>
        {value}
        {unit}
      </output>
    </label>
  );
}

function ThemeCard({
  theme,
  selected,
  onSelect,
}: {
  theme: ThemeDef;
  selected: boolean;
  onSelect: () => void;
}) {
  const t = theme.tokens;
  return (
    <button
      role="radio"
      aria-checked={selected}
      className={`theme-card${selected ? ' on' : ''}`}
      onClick={onSelect}
    >
      <span
        className="theme-swatch"
        style={{ background: t['background-gradient'] ?? t['color-bg'] }}
        aria-hidden
      >
        <span style={{ background: t['color-surface'], color: t['color-text'] }}>Aa</span>
        <span style={{ background: t['color-accent'] }} />
      </span>
      <span className="theme-name">{theme.name}</span>
    </button>
  );
}

const slug = (name: string) =>
  name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '') || 'theme';

/// Appearance: light/dark, themes for each, accent, background, panels, shape, density, type
/// and artwork size; every change shows at once.
export function AppearanceSettings() {
  const appearance = useLook((s) => s.appearance);
  const look = normaliseLook(appearance.look);
  const saved = useLook((s) => s.saved);
  const shownScheme = useLook((s) => s.scheme);
  const showToast = useStore((s) => s.showToast);
  const [slot, setSlot] = useState<Scheme>(shownScheme);
  const [editing, setEditing] = useState<ThemeDef | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  // From the store, not this render: imports and saves finish after other changes.
  const change = (patch: Partial<Appearance>) =>
    changeAppearance({ ...useLook.getState().appearance, ...patch });
  const setLook = (patch: Partial<Look>) =>
    change({ look: { ...normaliseLook(useLook.getState().appearance.look), ...patch } });

  const all = [...BUILT_IN_THEMES, ...saved];
  const chosenId = slot === 'dark' ? look.darkTheme : look.lightTheme;
  const chosen = all.find((t) => t.id === chosenId) ?? defaultTheme(slot);
  const choose = (id: string) => setLook(slot === 'dark' ? { darkTheme: id } : { lightTheme: id });

  const importFile = async (file: File) => {
    const text = await file.text();
    const added = await attempt(api.importTheme(text));
    if (!added) return;
    await reloadThemes();
    choose(added.id);
    showToast(`Added the theme “${added.theme.name}”.`);
  };

  const exportChosen = async () => {
    const file = `${slug(chosen.name)}.cadenza-theme.json`;
    const path = isAndroid
      ? `/storage/emulated/0/Download/${file}`
      : await saveDialog({
          defaultPath: file,
          filters: [{ name: 'Cadenza theme', extensions: ['json'] }],
        });
    if (!path) return;
    const where = await attempt(
      api.exportTheme({ name: chosen.name, scheme: chosen.scheme, tokens: chosen.tokens }, path),
    );
    if (where) showToast(`Saved the theme to ${where}.`);
  };

  const deleteChosen = async () => {
    if (chosen.builtIn) return;
    if (!(await attempt(api.deleteTheme(chosen.id)))) return;
    await reloadThemes();
    choose(slot === 'dark' ? 'default-dark' : 'default-light');
  };

  return (
    <>
      <div className="settings-group">
        <h2>Appearance</h2>
        <Segmented
          label="Appearance"
          value={appearance.theme}
          options={MODES.map((m) => ({ value: m.theme, label: m.label }))}
          onChange={(theme) => change({ theme })}
        />
        <p className="muted small">Auto follows your desktop’s light or dark setting.</p>
      </div>

      <div className="settings-group">
        <h2>Theme</h2>
        <div className="action-row">
          <Segmented
            label="Theme for"
            value={slot}
            options={[
              { value: 'light', label: 'When light' },
              { value: 'dark', label: 'When dark' },
            ]}
            onChange={setSlot}
          />
        </div>
        <div className="theme-cards" role="radiogroup" aria-label={`Theme when ${slot}`}>
          {all.map((t) => (
            <ThemeCard
              key={t.id}
              theme={t}
              selected={t.id === chosen.id}
              onSelect={() => choose(t.id)}
            />
          ))}
        </div>
        <div className="action-row">
          {chosen.builtIn ? (
            <button className="pill" onClick={() => setEditing(chosen)}>
              <Copy size={14} aria-hidden /> Duplicate and Edit
            </button>
          ) : (
            <button className="pill" onClick={() => setEditing(chosen)}>
              <Pencil size={14} aria-hidden /> Edit
            </button>
          )}
          <button
            className="pill"
            onClick={() => setEditing({ ...chosen, id: '', name: 'New Theme', builtIn: false })}
          >
            <Plus size={14} aria-hidden /> New
          </button>
          <button className="pill" onClick={() => fileInput.current?.click()}>
            <Upload size={14} aria-hidden /> Import…
          </button>
          <button className="pill" onClick={() => void exportChosen()}>
            <Download size={14} aria-hidden /> Export
          </button>
          {!chosen.builtIn && (
            <button className="pill" onClick={() => void deleteChosen()}>
              <Trash2 size={14} aria-hidden /> Delete
            </button>
          )}
          <input
            ref={fileInput}
            type="file"
            accept=".json,application/json"
            hidden
            onChange={(e) => {
              const f = e.target.files?.[0];
              e.target.value = '';
              if (f) void importFile(f);
            }}
          />
        </div>
        {isAndroid && <p className="muted small">Exported themes go to your Download folder.</p>}
      </div>

      <div className="settings-group">
        <h2>Accent Colour</h2>
        <Segmented
          label="Accent colour"
          value={look.accentSource}
          options={[
            { value: 'theme', label: 'Theme' },
            { value: 'custom', label: 'Custom' },
            { value: 'artwork', label: 'From Artwork' },
          ]}
          onChange={(accentSource) => setLook({ accentSource })}
        />
        <div className="accent-swatches" role="radiogroup" aria-label="Accent presets">
          {ACCENT_PRESETS.map((p) => {
            const on = look.accentSource === 'custom' && look.accent === p.color;
            return (
              <button
                key={p.color}
                role="radio"
                aria-checked={on}
                aria-label={p.name}
                title={p.name}
                className={`accent-swatch${on ? ' on' : ''}`}
                style={{ background: p.color }}
                onClick={() => setLook({ accentSource: 'custom', accent: p.color })}
              />
            );
          })}
          <label className="accent-custom" title="Any colour">
            <input
              type="color"
              aria-label="Custom accent colour"
              value={look.accent}
              onChange={(e) => setLook({ accentSource: 'custom', accent: e.target.value })}
            />
          </label>
        </div>
        <p className="muted small">
          From Artwork takes the most colourful part of the playing album’s cover, adjusted to stay
          readable.
        </p>
      </div>

      <div className="settings-group">
        <h2>Background and Panels</h2>
        <div className="form-grid">
          <span>Background</span>
          <Segmented
            label="Background"
            value={look.background}
            options={[
              { value: 'solid', label: 'Solid' },
              { value: 'gradient', label: 'Gradient' },
              { value: 'artwork', label: 'Artwork Tint' },
            ]}
            onChange={(background) => setLook({ background })}
          />
          <span>Panels</span>
          <Segmented
            label="Panels"
            value={look.panels}
            options={[
              { value: 'glass', label: 'Frosted Glass' },
              { value: 'liquid', label: 'Liquid Glass' },
              { value: 'solid', label: 'Solid' },
            ]}
            onChange={(panels) => setLook({ panels })}
          />
        </div>
        <Slider
          label="Blur"
          value={look.blur}
          min={0}
          max={60}
          unit=" px"
          disabled={look.panels === 'solid'}
          onChange={(blur) => setLook({ blur })}
        />
        <Slider
          label="Transparency"
          value={look.transparency}
          min={0}
          max={90}
          unit=" %"
          disabled={look.panels === 'solid'}
          onChange={(transparency) => setLook({ transparency })}
        />
        <p className="muted small">
          The gradient comes from the theme (edit it in the theme editor). Artwork Tint colours the
          background with the playing album’s cover.
        </p>
        {!isAndroid && (
          <>
            <label className="check-row">
              <input
                type="checkbox"
                checked={look.windowGlass}
                disabled={look.panels === 'solid'}
                onChange={(e) => setLook({ windowGlass: e.target.checked })}
              />
              See-through window
            </label>
            <p className="muted small">
              Lets the desktop show through the app, blurred by your desktop’s compositor: on KDE,
              KWin’s Blur or the Force Blur effect (whose refraction setting bends the edges).
              Without such an effect the desktop shows through unblurred.
            </p>
          </>
        )}
        <label className="check-row">
          <input
            type="checkbox"
            checked={appearance.reduceTransparency}
            onChange={(e) => change({ reduceTransparency: e.target.checked })}
          />
          Reduce transparency (solid sidebar, player bar and menus)
        </label>
        <label className="check-row">
          <input
            type="checkbox"
            checked={appearance.reduceMotion}
            onChange={(e) => change({ reduceMotion: e.target.checked })}
          />
          Reduce motion (no animations or moving backgrounds)
        </label>
        <p className="muted small">
          When your desktop asks for reduced motion or transparency, Cadenza follows it even with
          these off.
        </p>
      </div>

      <div className="settings-group">
        <h2>Shape and Text</h2>
        <div className="form-grid">
          <span>Density</span>
          <Segmented
            label="Density"
            value={look.density}
            options={[
              { value: 'compact', label: 'Compact' },
              { value: 'comfortable', label: 'Comfortable' },
              { value: 'spacious', label: 'Spacious' },
            ]}
            onChange={(density) => setLook({ density })}
          />
          <span>Artwork size</span>
          <Segmented
            label="Artwork size"
            value={look.artworkSize}
            options={[
              { value: 'small', label: 'Small' },
              { value: 'medium', label: 'Medium' },
              { value: 'large', label: 'Large' },
            ]}
            onChange={(artworkSize) => setLook({ artworkSize })}
          />
          <label htmlFor="look-font">Font</label>
          <div className="action-row">
            <Select<Look['font']>
              id="look-font"
              value={look.font}
              options={[
                { value: 'inter', label: 'Theme font (Inter unless the theme sets one)' },
                { value: 'system', label: 'System' },
                { value: 'serif', label: 'Serif' },
                { value: 'rounded', label: 'Rounded' },
                { value: 'mono', label: 'Monospace' },
                { value: 'custom', label: 'Other installed font…' },
              ]}
              onChange={(font) => setLook({ font })}
            />
            {look.font === 'custom' && (
              <input
                type="text"
                aria-label="Font name"
                placeholder="Font name, e.g. Noto Sans"
                value={look.customFont}
                onChange={(e) => setLook({ customFont: e.target.value })}
              />
            )}
          </div>
        </div>
        <Slider
          label="Text size"
          value={look.fontSize}
          min={80}
          max={140}
          step={5}
          unit=" %"
          onChange={(fontSize) => setLook({ fontSize })}
        />
        <Slider
          label="Corner rounding"
          value={look.radius}
          min={0}
          max={200}
          step={10}
          unit=" %"
          onChange={(radius) => setLook({ radius })}
        />
        <div className="action-row">
          <button
            className="pill"
            onClick={() =>
              setLook({
                ...DEFAULT_LOOK,
                lightTheme: look.lightTheme,
                darkTheme: look.darkTheme,
              })
            }
          >
            Reset Look
          </button>
          <span className="muted small">Keeps your themes; resets everything else here.</span>
        </div>
      </div>

      {editing && (
        <ThemeEditor
          start={editing}
          onClose={(id) => {
            setEditing(null);
            if (id) choose(id);
          }}
        />
      )}
    </>
  );
}
