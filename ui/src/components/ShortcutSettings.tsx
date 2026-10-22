import { useEffect, useState } from 'react';
import { updateLook, useLook } from '../hooks/useAppearance';
import { ACTIONS, FIXED_KEYS, GLOBAL_ACTIONS, bindings, comboOf, showCombo } from '../shortcuts';

/// Keyboard shortcuts: every action's keys, changed by pressing new ones.
export function ShortcutSettings() {
  const overrides = useLook((s) => s.look.shortcuts);
  const keys = bindings(overrides);
  const [recording, setRecording] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const assign = (id: string, combo: string) => {
    const next: Record<string, string> = { ...useLook.getState().look.shortcuts, [id]: combo };
    // A combination does one thing: take it from any other action.
    const taken = ACTIONS.find((a) => a.id !== id && combo && keys[a.id] === combo);
    if (taken) {
      next[taken.id] = '';
      setNote(`${showCombo(combo)} was used for “${taken.action}”, which now has no shortcut.`);
    } else setNote(null);
    // Back to the default: store nothing.
    const stored = Object.fromEntries(
      Object.entries(next).filter(([k, v]) => ACTIONS.find((a) => a.id === k)?.keys !== v),
    );
    updateLook({ shortcuts: stored });
  };

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === 'Escape') {
        setRecording(null);
        return;
      }
      const combo = comboOf(e);
      if (!combo) return;
      assign(recording, combo);
      setRecording(null);
    };
    // Capture: before the app's own shortcut handler sees the keys.
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  });

  return (
    <div className="settings-group">
      <h2>Keyboard Shortcuts</h2>
      <table className="shortcut-table">
        <tbody>
          {ACTIONS.map((a) => (
            <tr key={a.id}>
              <td>{a.action}</td>
              <td>
                <button
                  className={`shortcut-key ${recording === a.id ? 'recording' : ''}`}
                  aria-label={`${a.action}: ${showCombo(keys[a.id] ?? '')}. Change`}
                  onClick={() => setRecording(recording === a.id ? null : a.id)}
                >
                  {recording === a.id ? (
                    'Press keys… (Esc cancels)'
                  ) : (
                    <kbd>{showCombo(keys[a.id] ?? '')}</kbd>
                  )}
                </button>
              </td>
              <td>
                {keys[a.id] && (
                  <button className="link" onClick={() => assign(a.id, '')}>
                    Clear
                  </button>
                )}{' '}
                {keys[a.id] !== a.keys && (
                  <button className="link" onClick={() => assign(a.id, a.keys)}>
                    Default ({showCombo(a.keys)})
                  </button>
                )}
              </td>
            </tr>
          ))}
          {FIXED_KEYS.map((s) => (
            <tr key={s.keys}>
              <td>{s.action}</td>
              <td>
                <kbd>{s.keys}</kbd>
              </td>
              <td className="muted small">Fixed</td>
            </tr>
          ))}
        </tbody>
      </table>
      {note && (
        <p className="small" role="status">
          {note}
        </p>
      )}
      <div className="action-row">
        <button
          className="pill"
          disabled={Object.keys(overrides).length === 0}
          onClick={() => {
            updateLook({ shortcuts: {} });
            setNote(null);
          }}
        >
          Reset All Shortcuts
        </button>
      </div>
      <p className="muted small">
        Click a shortcut, then press the new keys. Tab moves between controls; Enter or Space
        activates them. Media keys on your keyboard work through the desktop’s media controls.
      </p>
    </div>
  );
}

/// Keys that work while other apps have focus. Linux (X11) only; Wayland doesn't allow it.
export function GlobalShortcutSettings() {
  const keys = useLook((s) => s.look.globalShortcuts);
  const result = useLook((s) => s.globalShortcuts);
  const [recording, setRecording] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  const assign = (id: string, combo: string) => {
    const next = Object.fromEntries(
      Object.entries({ ...useLook.getState().look.globalShortcuts, [id]: combo }).filter(
        ([k, v]) => v && (k === id || v !== combo),
      ),
    );
    updateLook({ globalShortcuts: next });
  };

  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === 'Escape') {
        setRecording(null);
        return;
      }
      const combo = comboOf(e);
      if (!combo) return;
      // Plain keys system-wide would stop them working in every other app.
      if (!/^(Ctrl|Alt)\+/.test(combo) && !/^(Shift\+)?(F\d+|Media\w+|Audio\w+)$/.test(combo)) {
        setNote(
          'Use Ctrl or Alt with the key (or a function or media key), so typing elsewhere still works.',
        );
        return;
      }
      setNote(null);
      assign(recording, combo);
      setRecording(null);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  });

  const failed = new Map(result?.failed ?? []);
  return (
    <div className="settings-group">
      <h2>System-Wide Shortcuts</h2>
      {result?.wayland && (
        <p className="small safe-mode" role="status">
          This desktop uses Wayland, which doesn’t let apps take keys system-wide. Instead, add
          shortcuts in your desktop’s keyboard settings that run <code>playerctl play-pause</code>,{' '}
          <code>playerctl next</code> or <code>playerctl previous</code>: they control Cadenza.
        </p>
      )}
      <table className="shortcut-table">
        <tbody>
          {GLOBAL_ACTIONS.map((id) => {
            const action = ACTIONS.find((a) => a.id === id);
            if (!action) return null;
            return (
              <tr key={id}>
                <td>{action.action}</td>
                <td>
                  <button
                    className={`shortcut-key ${recording === id ? 'recording' : ''}`}
                    aria-label={`${action.action} system-wide: ${showCombo(keys[id] ?? '')}. Change`}
                    onClick={() => setRecording(recording === id ? null : id)}
                  >
                    {recording === id ? (
                      'Press keys… (Esc cancels)'
                    ) : (
                      <kbd>{showCombo(keys[id] ?? '')}</kbd>
                    )}
                  </button>
                  {failed.has(id) && <span className="warn small"> {failed.get(id)}</span>}
                </td>
                <td>
                  {keys[id] && (
                    <button className="link" onClick={() => assign(id, '')}>
                      Clear
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {note && (
        <p className="small" role="status">
          {note}
        </p>
      )}
      <p className="muted small">
        These work while another app has focus (on X11 desktops). None are set at first, so no other
        app loses its keys. Media keys on your keyboard already work through the desktop’s media
        controls.
      </p>
    </div>
  );
}
