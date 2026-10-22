import { useState } from 'react';
import { updateLook, useLook } from '../hooks/useAppearance';

/// The owner's CSS, applied after the theme. Desktop only.
export function CustomCssSettings() {
  const saved = useLook((s) => s.look.customCss);
  const safeMode = useLook((s) => s.safeMode);
  const [draft, setDraft] = useState(saved);
  const changed = draft !== saved;

  return (
    <div className="settings-group">
      <h2>Custom CSS</h2>
      {safeMode && (
        <p className="small safe-mode" role="status">
          Safe mode: your CSS is off until Cadenza restarts (without --safe-mode). Fix or clear it here.
        </p>
      )}
      <textarea
        className="css-editor"
        aria-label="Custom CSS"
        spellCheck={false}
        rows={10}
        value={draft}
        placeholder={
          ':root {\n  --color-accent: #2f7cf6;\n  --radius-m: 4px;\n}\n.sidebar h3 {\n  text-transform: uppercase;\n}'
        }
        onChange={(e) => setDraft(e.target.value)}
      />
      <div className="action-row">
        <button
          className="pill primary"
          disabled={!changed}
          onClick={() => updateLook({ customCss: draft })}
        >
          Apply
        </button>
        <button className="pill" disabled={!changed} onClick={() => setDraft(saved)}>
          Revert
        </button>
        <button
          className="pill"
          disabled={!saved && !draft}
          onClick={() => {
            setDraft('');
            updateLook({ customCss: '' });
          }}
        >
          Clear
        </button>
      </div>
      <p className="muted small">
        Applied after the theme, so it can change any design token (<code>--color-*</code>,{' '}
        <code>--radius-*</code>, <code>--space-*</code>, <code>--font-*</code>…) or style any part
        of the app. Images and fonts can only come from Cadenza itself or <code>data:</code>{' '}
        addresses, never the web. If your CSS hides the way back here, press Ctrl+Alt+Shift+S, or
        start Cadenza with <code>cadenza --safe-mode</code>.
      </p>
    </div>
  );
}
