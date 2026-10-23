import {
  Clock,
  Disc3,
  FolderSync,
  House,
  Library,
  ListMusic,
  Mic2,
  Search,
  Settings,
} from 'lucide-react';
import { useLook } from '../hooks/useAppearance';
import { type Route, useStore } from '../store';
import { TAB_LABELS, type TabId } from '../theme/layout';

const ICONS: Record<TabId, typeof House> = {
  home: House,
  library: Library,
  search: Search,
  albums: Disc3,
  artists: Mic2,
  songs: ListMusic,
  recent: Clock,
  sync: FolderSync,
  settings: Settings,
};

const ROUTES: Record<TabId, Route> = {
  home: { name: 'home' },
  library: { name: 'library' },
  search: { name: 'search', query: '' },
  albums: { name: 'albums' },
  artists: { name: 'artists' },
  songs: { name: 'songs' },
  recent: { name: 'recent' },
  sync: { name: 'sync' },
  settings: { name: 'settings' },
};

/// The tab a page belongs to: its own if it has one, else Library (where everything is).
export function tabOf(route: Route, tabs: TabId[]): TabId {
  const own: Partial<Record<Route['name'], TabId>> = {
    home: 'home',
    library: 'library',
    search: 'search',
    albums: 'albums',
    album: 'albums',
    artists: 'artists',
    artist: 'artists',
    songs: 'songs',
    recent: 'recent',
    sync: 'sync',
    settings: 'settings',
  };
  const tab = own[route.name];
  return tab && tabs.includes(tab) ? tab : 'library';
}

/// The phone's tabs, in the owner's order (Settings → Layout → Phone).
export function PhoneTabBar() {
  const tabs = useLook((s) => s.look.layout.phoneTabs);
  const route = useStore((s) => s.route);
  const current = tabOf(route, tabs);
  return (
    <nav className="phone-tabs" aria-label="Tabs">
      {tabs.map((t) => {
        const Icon = ICONS[t];
        return (
          <button
            key={t}
            className={current === t ? 'on' : ''}
            aria-current={current === t ? 'page' : undefined}
            onClick={() => {
              const store = useStore.getState();
              // Tapping the tab you're on goes back to its top page.
              if (current === t && route.name !== ROUTES[t].name) store.navigate(ROUTES[t]);
              else if (current !== t) store.navigate(ROUTES[t]);
            }}
          >
            <Icon size={22} aria-hidden />
            <span>{TAB_LABELS[t]}</span>
          </button>
        );
      })}
    </nav>
  );
}
