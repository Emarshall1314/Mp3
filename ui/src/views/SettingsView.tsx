import { useEffect, useState } from 'react';
import { api } from '../api';
import { applyAppearance } from '../hooks/useAppearance';
import { attempt } from '../store';
import { SHORTCUTS } from '../shortcuts';
import type { Appearance, Theme } from '../types';

const THEMES: { theme: Theme; label: string }[] = [
  { theme: 'auto', label: 'Auto' },
  { theme: 'light', label: 'Light' },
  { theme: 'dark', label: 'Dark' },
];

/// Settings: appearance and the keyboard shortcut reference. Sound settings live in Sound.
export function SettingsView() {
  const [appearance, setAppearance] = useState<Appearance | null>(null);

  useEffect(() => {
    void api.settings().then((s) => setAppearance(s.appearance));
  }, []);

  if (!appearance) return <section className="view" aria-busy="true" />;
  const change = (patch: Partial<Appearance>) => {
    const next = { ...appearance, ...patch };
    setAppearance(next);
    applyAppearance(next);
    void attempt(api.setAppearance(next));
  };

  return (
    <section className="view scroll sound-view">
      <header className="view-header">
        <h1>Settings</h1>
      </header>

      <div className="settings-group">
        <h2>Appearance</h2>
        <div className="segmented" role="radiogroup" aria-label="Theme">
          {THEMES.map(({ theme, label }) => (
            <button
              key={theme}
              role="radio"
              aria-checked={appearance.theme === theme}
              className={appearance.theme === theme ? 'on' : ''}
              onClick={() => change({ theme })}
            >
              {label}
            </button>
          ))}
        </div>
        <p className="muted small">Auto follows your desktop’s light or dark setting.</p>
        <label className="check-row">
          <input
            type="checkbox"
            checked={appearance.reduceMotion}
            onChange={(e) => change({ reduceMotion: e.target.checked })}
          />
          Reduce motion (no animations or moving backgrounds)
        </label>
        <label className="check-row">
          <input
            type="checkbox"
            checked={appearance.reduceTransparency}
            onChange={(e) => change({ reduceTransparency: e.target.checked })}
          />
          Reduce transparency (solid sidebar, player bar and menus)
        </label>
        <p className="muted small">
          When your desktop asks for reduced motion or transparency, Cadenza follows it even with
          these off.
        </p>
      </div>

      <div className="settings-group">
        <h2>Keyboard Shortcuts</h2>
        <table className="shortcut-table">
          <tbody>
            {SHORTCUTS.map((s) => (
              <tr key={s.keys}>
                <td>
                  <kbd>{s.keys}</kbd>
                </td>
                <td>{s.action}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="muted small">Tab moves between controls; Enter or Space activates them.</p>
      </div>
    </section>
  );
}
