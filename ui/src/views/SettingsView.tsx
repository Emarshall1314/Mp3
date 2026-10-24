import { useEffect, useState } from 'react';
import {
  ChevronRight,
  Code2,
  FolderOpen,
  FolderSync,
  Info,
  Keyboard,
  LayoutTemplate,
  Palette,
  SlidersHorizontal,
  Trash2,
} from 'lucide-react';
import { AppearanceSettings } from '../components/AppearanceSettings';
import { LayoutSettings } from '../components/LayoutSettings';
import { GlobalShortcutSettings, ShortcutSettings } from '../components/ShortcutSettings';
import { CustomCssSettings } from '../components/CustomCssSettings';
import { api, isAndroid } from '../api';
import { type Route, type SettingsSection, useStore } from '../store';
import type { BuildInfo } from '../types';

interface Entry {
  label: string;
  /// A short line under the label.
  detail: string;
  icon: typeof Palette;
  /// The colour of the icon's tile, as in iOS Settings.
  tint: string;
  to: Route;
}

const TITLES: Record<SettingsSection, string> = {
  appearance: 'Appearance',
  layout: 'Layout',
  shortcuts: 'Keyboard',
  css: 'Custom CSS',
  about: 'About',
};

function entries(): Entry[][] {
  const look: Entry[] = [
    {
      label: 'Appearance',
      detail: 'Theme, accent, glass, text',
      icon: Palette,
      tint: '#5e5ce6',
      to: { name: 'settings', section: 'appearance' },
    },
    {
      label: 'Layout',
      detail: 'Tabs, player, Home shelves',
      icon: LayoutTemplate,
      tint: '#0a84ff',
      to: { name: 'settings', section: 'layout' },
    },
  ];
  if (!isAndroid)
    look.push(
      {
        label: 'Keyboard',
        detail: 'Shortcuts in the app and system-wide',
        icon: Keyboard,
        tint: '#8e8e93',
        to: { name: 'settings', section: 'shortcuts' },
      },
      {
        label: 'Custom CSS',
        detail: 'Your own styles over the theme',
        icon: Code2,
        tint: '#30b0c7',
        to: { name: 'settings', section: 'css' },
      },
    );
  return [
    look,
    [
      {
        label: 'Sound',
        detail: 'Output, EQ, ReplayGain, crossfade',
        icon: SlidersHorizontal,
        tint: '#ff375f',
        to: { name: 'sound' },
      },
      {
        label: 'Music Folders',
        detail: 'Where your music is',
        icon: FolderOpen,
        tint: '#ff9f0a',
        to: { name: 'folders' },
      },
      {
        label: 'Sync',
        detail: 'Other devices',
        icon: FolderSync,
        tint: '#30d158',
        to: { name: 'sync' },
      },
      {
        label: 'Trash',
        detail: 'Songs kept for 30 days',
        icon: Trash2,
        tint: '#8e8e93',
        to: { name: 'trash' },
      },
    ],
    [
      {
        label: 'About',
        detail: 'Version',
        icon: Info,
        tint: '#64d2ff',
        to: { name: 'settings', section: 'about' },
      },
    ],
  ];
}

/// Settings, as on an iPhone: a list of sections in rounded groups, each opening its own page
/// (with a back button and swipe back). Settings that only apply to the desktop or the phone
/// show only there.
export function SettingsView({ section }: { section?: SettingsSection }) {
  if (section) {
    return (
      <section className="view scroll settings-page" data-section={section}>
        <header className="view-header">
          <h1>{TITLES[section]}</h1>
        </header>
        {section === 'appearance' && <AppearanceSettings />}
        {section === 'layout' && <LayoutSettings />}
        {section === 'shortcuts' && (
          <>
            <ShortcutSettings />
            <GlobalShortcutSettings />
          </>
        )}
        {section === 'css' && <CustomCssSettings />}
        {section === 'about' && <About />}
      </section>
    );
  }
  const navigate = useStore.getState().navigate;
  return (
    <section className="view scroll settings-page">
      <header className="view-header">
        <h1>Settings</h1>
      </header>
      {entries().map((group, i) => (
        <ul key={i} className="settings-list">
          {group.map((e) => (
            <li key={e.label}>
              <button className="settings-row" onClick={() => navigate(e.to)}>
                <span className="settings-icon" style={{ background: e.tint }} aria-hidden>
                  <e.icon size={17} />
                </span>
                <span className="settings-row-text">
                  <span className="title">{e.label}</span>
                  <span className="subtitle">{e.detail}</span>
                </span>
                <ChevronRight size={18} className="chevron" aria-hidden />
              </button>
            </li>
          ))}
        </ul>
      ))}
    </section>
  );
}

function About() {
  const [info, setInfo] = useState<BuildInfo | null>(null);
  useEffect(() => {
    void api.buildInfo().then(setInfo);
  }, []);
  return (
    <div className="settings-group">
      <h2>Cadenza</h2>
      {info && (
        <dl className="about-list">
          <dt>Version</dt>
          <dd>
            {info.version}
            {info.debug ? ' (development build)' : ''}
          </dd>
          <dt>Platform</dt>
          <dd>
            {info.targetOs} · {info.targetArch}
          </dd>
        </dl>
      )}
      <p className="muted small">
        A music player for your own files. Your library, listening and settings stay on your
        devices.
      </p>
    </div>
  );
}
