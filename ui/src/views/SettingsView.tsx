import { AppearanceSettings } from '../components/AppearanceSettings';
import { LayoutSettings } from '../components/LayoutSettings';
import { ShortcutSettings } from '../components/ShortcutSettings';
import { CustomCssSettings } from '../components/CustomCssSettings';
import { isAndroid } from '../api';

/// Settings: appearance, themes, layout, shortcuts and custom CSS. Sound settings live in Sound.
export function SettingsView() {
  return (
    <section className="view scroll sound-view">
      <header className="view-header">
        <h1>Settings</h1>
      </header>

      <AppearanceSettings />
      <LayoutSettings />
      <ShortcutSettings />
      {!isAndroid && <CustomCssSettings />}
    </section>
  );
}
