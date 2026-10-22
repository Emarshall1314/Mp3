import { AppearanceSettings } from '../components/AppearanceSettings';
import { SHORTCUTS } from '../shortcuts';

/// Settings: appearance, themes and the keyboard shortcut reference. Sound settings live in Sound.
export function SettingsView() {
  return (
    <section className="view scroll sound-view">
      <header className="view-header">
        <h1>Settings</h1>
      </header>

      <AppearanceSettings />

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
