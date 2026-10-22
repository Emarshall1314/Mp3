import { useEffect, useState } from 'react';
import { Clock, Disc3, FolderOpen, ListMusic, Mic2, Search, Sparkles, X } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore, type Route } from '../store';
import type { SavedSearch } from '../types';

const LIBRARY: { route: Route; label: string; icon: typeof Clock }[] = [
  { route: { name: 'recent' }, label: 'Recently Added', icon: Clock },
  { route: { name: 'artists' }, label: 'Artists', icon: Mic2 },
  { route: { name: 'albums' }, label: 'Albums', icon: Disc3 },
  { route: { name: 'songs' }, label: 'Songs', icon: ListMusic },
];

export function Sidebar() {
  const route = useStore((s) => s.route);
  const navigate = useStore((s) => s.navigate);
  const libraryVersion = useStore((s) => s.libraryVersion);
  const scan = useStore((s) => s.scan);
  const query = route.name === 'search' ? route.query : '';
  const [saved, setSaved] = useState<SavedSearch[]>([]);

  useEffect(() => {
    void api.savedSearches().then(setSaved);
  }, [libraryVersion, route]);

  // Instant search: every keystroke updates the results view.
  const onSearch = (q: string) => {
    if (q) {
      const r: Route = { name: 'search', query: q };
      if (route.name === 'search') useStore.setState({ route: r });
      else navigate(r);
    } else if (route.name === 'search') {
      useStore.getState().goBack();
    }
  };

  return (
    <nav className="sidebar" aria-label="Library">
      <label className="search-field">
        <Search size={14} aria-hidden />
        <input
          type="search"
          placeholder="Search"
          aria-label="Search"
          value={query}
          onChange={(e) => onSearch(e.target.value)}
          onKeyDown={(e) => e.key === 'Escape' && onSearch('')}
        />
      </label>

      <h3>Library</h3>
      <ul>
        {LIBRARY.map(({ route: r, label, icon: Icon }) => (
          <li key={label}>
            <button
              className={`nav-item ${route.name === r.name || (route.name === 'album' && r.name === 'albums') || (route.name === 'artist' && r.name === 'artists') ? 'active' : ''}`}
              onClick={() => navigate(r)}
            >
              <Icon size={16} aria-hidden />
              {label}
            </button>
          </li>
        ))}
      </ul>

      {saved.length > 0 && (
        <>
          <h3>Saved Searches</h3>
          <ul>
            {saved.map((s) => (
              <li key={s.id} className="nav-row">
                <button
                  className={`nav-item ${route.name === 'search' && route.query === s.query ? 'active' : ''}`}
                  onClick={() => navigate({ name: 'search', query: s.query })}
                  title={s.query}
                >
                  <Sparkles size={16} aria-hidden />
                  {s.name}
                </button>
                <button
                  className="icon-button small remove"
                  aria-label={`Delete saved search ${s.name}`}
                  onClick={() =>
                    void attempt(api.deleteSavedSearch(s.id)).then(() =>
                      api.savedSearches().then(setSaved),
                    )
                  }
                >
                  <X size={12} />
                </button>
              </li>
            ))}
          </ul>
        </>
      )}

      <div className="sidebar-footer">
        <button
          className={`nav-item ${route.name === 'folders' ? 'active' : ''}`}
          onClick={() => navigate({ name: 'folders' })}
        >
          <FolderOpen size={16} aria-hidden />
          Music Folders
        </button>
        {scan.running && scan.progress && (
          <p className="scan-status" role="status">
            {scan.progress.phase === 'reading'
              ? `Scanning ${scan.progress.read.toLocaleString()} of ${scan.progress.toRead.toLocaleString()}`
              : scan.progress.phase === 'walking'
                ? 'Looking for music…'
                : 'Updating library…'}
          </p>
        )}
      </div>
    </nav>
  );
}
