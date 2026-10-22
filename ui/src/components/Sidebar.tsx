import { useEffect, useRef, useState } from 'react';
import {
  Clock,
  Disc3,
  FolderOpen,
  House,
  ListMusic,
  Mic2,
  Search,
  Settings,
  SlidersHorizontal,
  Sparkles,
  X,
} from 'lucide-react';
import { api } from '../api';
import { attempt, useStore, type Route } from '../store';
import type { SavedSearch } from '../types';
import { updateLayout, useLook } from '../hooks/useAppearance';
import { NAV_LABELS, type NavId } from '../theme/layout';

export const NAV_ICONS: Record<NavId, typeof Clock> = {
  home: House,
  recent: Clock,
  artists: Mic2,
  albums: Disc3,
  songs: ListMusic,
};

/// The sidebar's right edge: drag (or arrow keys) to resize, double-click to collapse.
function ResizeHandle() {
  const layout = useLook((s) => s.look.layout);
  const drag = (e: React.PointerEvent) => {
    e.preventDefault();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) => {
      const width = Math.round(Math.min(400, Math.max(180, m.clientX)));
      updateLayout({ sidebarWidth: width, sidebarCollapsed: m.clientX < 120 });
    };
    const up = () => {
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
  };
  return (
    <div
      className="sidebar-resize"
      role="separator"
      aria-orientation="vertical"
      aria-label="Sidebar width"
      aria-valuemin={180}
      aria-valuemax={400}
      aria-valuenow={layout.sidebarCollapsed ? 64 : layout.sidebarWidth}
      tabIndex={0}
      title="Drag to resize; double-click to collapse"
      onPointerDown={drag}
      onDoubleClick={() => updateLayout({ sidebarCollapsed: !layout.sidebarCollapsed })}
      onKeyDown={(e) => {
        const by = e.key === 'ArrowLeft' ? -16 : e.key === 'ArrowRight' ? 16 : 0;
        if (e.key === 'Enter') updateLayout({ sidebarCollapsed: !layout.sidebarCollapsed });
        else if (by)
          updateLayout({
            sidebarCollapsed: false,
            sidebarWidth: Math.min(400, Math.max(180, layout.sidebarWidth + by)),
          });
        else return;
        e.preventDefault();
      }}
    />
  );
}

export function Sidebar() {
  const route = useStore((s) => s.route);
  const navigate = useStore((s) => s.navigate);
  const libraryVersion = useStore((s) => s.libraryVersion);
  const scan = useStore((s) => s.scan);
  const edit = useStore((s) => s.edit);
  const query = route.name === 'search' ? route.query : '';
  const [saved, setSaved] = useState<SavedSearch[]>([]);
  const layout = useLook((s) => s.look.layout);
  const collapsed = layout.sidebarCollapsed;
  const searchInput = useRef<HTMLInputElement>(null);

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
      <label
        className="search-field"
        title={collapsed ? 'Search' : undefined}
        onClick={() => {
          if (!collapsed) return;
          updateLayout({ sidebarCollapsed: false });
          setTimeout(() => searchInput.current?.focus());
        }}
      >
        <Search size={14} aria-hidden />
        <input
          ref={searchInput}
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
        {layout.nav.map((id) => {
          const Icon = NAV_ICONS[id];
          const active =
            route.name === id ||
            (route.name === 'album' && id === 'albums') ||
            (route.name === 'artist' && id === 'artists');
          return (
            <li key={id}>
              <button
                className={`nav-item ${active ? 'active' : ''}`}
                title={collapsed ? NAV_LABELS[id] : undefined}
                aria-label={collapsed ? NAV_LABELS[id] : undefined}
                onClick={() => navigate({ name: id })}
              >
                <Icon size={16} aria-hidden />
                <span className="nav-label">{NAV_LABELS[id]}</span>
              </button>
            </li>
          );
        })}
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
                  <span className="nav-label">{s.name}</span>
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
          className={`nav-item ${route.name === 'sound' ? 'active' : ''}`}
          title={collapsed ? 'Sound' : undefined}
          aria-label={collapsed ? 'Sound' : undefined}
          onClick={() => navigate({ name: 'sound' })}
        >
          <SlidersHorizontal size={16} aria-hidden />
          <span className="nav-label">Sound</span>
        </button>
        <button
          className={`nav-item ${route.name === 'folders' ? 'active' : ''}`}
          title={collapsed ? 'Music Folders' : undefined}
          aria-label={collapsed ? 'Music Folders' : undefined}
          onClick={() => navigate({ name: 'folders' })}
        >
          <FolderOpen size={16} aria-hidden />
          <span className="nav-label">Music Folders</span>
        </button>
        <button
          className={`nav-item ${route.name === 'settings' ? 'active' : ''}`}
          title={collapsed ? 'Settings' : undefined}
          aria-label={collapsed ? 'Settings' : undefined}
          onClick={() => navigate({ name: 'settings' })}
        >
          <Settings size={16} aria-hidden />
          <span className="nav-label">Settings</span>
        </button>
        {edit?.running && (
          <p className="scan-status" role="status">
            Saving tags {Math.min(edit.done + 1, edit.total).toLocaleString()} of{' '}
            {edit.total.toLocaleString()}
            <button className="link" onClick={() => void attempt(api.cancelEdit())}>
              Stop
            </button>
          </p>
        )}
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
      <ResizeHandle />
    </nav>
  );
}
