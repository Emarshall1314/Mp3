import { useEffect, useRef, useState } from 'react';
import { AlertTriangle, Check, X } from 'lucide-react';
import { api } from '../api';
import { previewTheme, reloadThemes, useLook } from '../hooks/useAppearance';
import { useModal } from '../hooks/useModal';
import { attempt } from '../store';
import {
  type Scheme,
  type ThemeDef,
  THEME_TOKENS,
  contrastChecks,
  over,
  parseColor,
} from '../theme/look';
import { SegmentLens } from './SegmentLens';

const hex = (css: string) => {
  const c = parseColor(css);
  if (!c) return null;
  const rgb = over(c, [255, 255, 255]);
  return '#' + rgb.map((x) => Math.round(x).toString(16).padStart(2, '0')).join('');
};

/// Edits a theme with every token visible and the whole app previewing it as it changes. Opens
/// on a copy; saving writes the owner's theme (`id`), or a new one when editing a built-in.
export function ThemeEditor({
  start,
  onClose,
}: {
  start: ThemeDef;
  onClose: (saved: string | null) => void;
}) {
  const [draft, setDraft] = useState<ThemeDef>(() =>
    start.builtIn ? { ...start, id: '', name: `${start.name} Copy`, builtIn: false } : start,
  );
  const ref = useRef<HTMLDivElement>(null);
  const cancel = () => {
    previewTheme(null);
    onClose(null);
  };
  useModal(true, ref, cancel);

  useEffect(() => {
    previewTheme(draft);
  }, [draft]);
  useEffect(() => () => previewTheme(null), []);

  const set = (name: string, value: string) =>
    setDraft((d) => ({ ...d, tokens: { ...d.tokens, [name]: value } }));
  const accentSource = useLook((s) => s.look.accentSource);
  const checks = contrastChecks(draft.tokens);
  const failing = checks.filter((c) => c.ratio < c.needed);
  // Colours the browser can't read would leave parts of the app without a colour.
  const invalid = THEME_TOKENS.filter(
    (t) => t.kind === 'color' && !parseColor(draft.tokens[t.name] ?? ''),
  ).map((t) => t.label);

  const save = async () => {
    const saved = await attempt(
      api.saveTheme(draft.id || null, {
        name: draft.name,
        scheme: draft.scheme,
        tokens: draft.tokens,
      }),
    );
    if (!saved) return;
    await reloadThemes();
    previewTheme(null);
    onClose(saved.id);
  };

  const groups = [...new Set(THEME_TOKENS.map((t) => t.group))];

  return (
    <div
      ref={ref}
      className="theme-editor"
      role="dialog"
      aria-modal="true"
      aria-label="Theme Editor"
    >
      <header className="sheet-header">
        <div className="sheet-title">
          <h2>{draft.id ? 'Edit Theme' : 'New Theme'}</h2>
          <p className="muted">The whole app shows your changes as you make them.</p>
        </div>
        <button className="icon-button" aria-label="Cancel" onClick={cancel}>
          <X size={18} />
        </button>
      </header>
      <div className="sheet-body">
        <div className="form-grid">
          <label htmlFor="theme-name">Name</label>
          <input
            id="theme-name"
            type="text"
            data-autofocus
            maxLength={60}
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
          />
          <span>Kind</span>
          <div className="segmented" role="radiogroup" aria-label="Kind">
            <SegmentLens />
            {(['light', 'dark'] as Scheme[]).map((s) => (
              <button
                key={s}
                role="radio"
                aria-checked={draft.scheme === s}
                className={draft.scheme === s ? 'on' : ''}
                onClick={() => setDraft({ ...draft, scheme: s })}
              >
                {s === 'light' ? 'Light' : 'Dark'}
              </button>
            ))}
          </div>
        </div>

        {accentSource !== 'theme' && (
          <p className="muted small">
            Your accent setting ({accentSource === 'custom' ? 'Custom' : 'From Artwork'}) is shown
            instead of this theme’s accent. Set Accent Colour to Theme to see it.
          </p>
        )}
        <section className="contrast" aria-live="polite">
          {invalid.length > 0 && (
            <p className="contrast-warning">
              <AlertTriangle size={14} aria-hidden /> Not a colour: {invalid.join(', ')}. Fix these
              to save the theme.
            </p>
          )}
          {failing.length === 0 ? (
            <p className="contrast-ok">
              <Check size={14} aria-hidden /> Text contrast meets WCAG AA.
            </p>
          ) : (
            failing.map((c) => (
              <p key={c.label} className="contrast-warning">
                <AlertTriangle size={14} aria-hidden /> {c.label}: {c.ratio.toFixed(1)}:1, needs{' '}
                {c.needed}:1 (WCAG AA)
              </p>
            ))
          )}
        </section>

        {groups.map((group) => (
          <fieldset key={group} className="token-group">
            <legend>{group}</legend>
            {THEME_TOKENS.filter((t) => t.group === group).map((t) => {
              const value = draft.tokens[t.name] ?? '';
              const swatch = t.kind === 'color' ? hex(value) : null;
              return (
                <div key={t.name} className="token-row">
                  <label htmlFor={`token-${t.name}`}>{t.label}</label>
                  {t.kind === 'color' && (
                    <input
                      type="color"
                      aria-label={`${t.label} colour`}
                      value={swatch ?? '#000000'}
                      onChange={(e) => {
                        // Keep the alpha of translucent colours.
                        const a = parseColor(value)?.[3] ?? 1;
                        const v = e.target.value;
                        if (a >= 1) set(t.name, v);
                        else {
                          const [r, g, b] = [1, 3, 5].map((i) => parseInt(v.slice(i, i + 2), 16));
                          set(t.name, `rgb(${r} ${g} ${b} / ${Math.round(a * 100)}%)`);
                        }
                      }}
                    />
                  )}
                  <input
                    id={`token-${t.name}`}
                    type="text"
                    spellCheck={false}
                    value={value}
                    onChange={(e) => set(t.name, e.target.value)}
                  />
                </div>
              );
            })}
          </fieldset>
        ))}
        <p className="muted small">
          Values are CSS: colours (#hex, rgb(), oklch()...), gradients, sizes and font names. Web
          addresses (url()) aren’t allowed: a theme never loads anything.
        </p>
      </div>
      <footer className="sheet-footer">
        <span />
        <button className="pill" onClick={cancel}>
          Cancel
        </button>
        <button
          className="pill primary"
          disabled={!draft.name.trim() || invalid.length > 0}
          onClick={() => void save()}
        >
          Save Theme
        </button>
      </footer>
    </div>
  );
}
