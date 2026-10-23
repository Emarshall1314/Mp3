import { useEffect, useState } from 'react';
import {
  ChevronRight,
  Clock,
  Disc3,
  FolderOpen,
  FolderSync,
  ListMusic,
  Music2,
  Mic2,
  Settings,
  SlidersHorizontal,
  Sparkles,
  Trash2,
} from 'lucide-react';
import { api } from '../api';
import { type Route, useStore } from '../store';
import type { SavedSearch } from '../types';

const ENTRIES: { label: string; route: Route; icon: typeof Clock }[] = [
  { label: 'Recently Added', route: { name: 'recent' }, icon: Clock },
  { label: 'Artists', route: { name: 'artists' }, icon: Mic2 },
  { label: 'Albums', route: { name: 'albums' }, icon: Disc3 },
  { label: 'Songs', route: { name: 'songs' }, icon: Music2 },
  { label: 'Playlists', route: { name: 'playlists' }, icon: ListMusic },
];

const MORE: { label: string; route: Route; icon: typeof Clock }[] = [
  { label: 'Music Folders', route: { name: 'folders' }, icon: FolderOpen },
  { label: 'Sync', route: { name: 'sync' }, icon: FolderSync },
  { label: 'Trash', route: { name: 'trash' }, icon: Trash2 },
  { label: 'Sound', route: { name: 'sound' }, icon: SlidersHorizontal },
  { label: 'Settings', route: { name: 'settings' }, icon: Settings },
];

function Row({ label, route, icon: Icon }: { label: string; route: Route; icon: typeof Clock }) {
  return (
    <li>
      <button className="library-row" onClick={() => useStore.getState().navigate(route)}>
        <Icon size={20} aria-hidden />
        <span>{label}</span>
        <ChevronRight size={18} aria-hidden className="chevron" />
      </button>
    </li>
  );
}

/// The phone's Library tab: everything the desktop sidebar holds.
export function LibraryView() {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [saved, setSaved] = useState<SavedSearch[]>([]);
  useEffect(() => {
    void api.savedSearches().then(setSaved);
  }, [libraryVersion]);
  return (
    <section className="view scroll library-view">
      <header className="view-header">
        <h1>Library</h1>
      </header>
      <ul className="library-list">
        {ENTRIES.map((e) => (
          <Row key={e.label} {...e} />
        ))}
      </ul>
      {saved.length > 0 && (
        <>
          <h2 className="section-title">Saved Searches</h2>
          <ul className="library-list">
            {saved.map((s) => (
              <Row
                key={s.id}
                label={s.name}
                route={{ name: 'search', query: s.query }}
                icon={Sparkles}
              />
            ))}
          </ul>
        </>
      )}
      <h2 className="section-title">More</h2>
      <ul className="library-list">
        {MORE.map((e) => (
          <Row key={e.label} {...e} />
        ))}
      </ul>
    </section>
  );
}
