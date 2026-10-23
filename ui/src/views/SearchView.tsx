import { useEffect, useState } from 'react';
import { Bookmark } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { SearchResults } from '../types';
import { Artwork } from '../components/Artwork';
import { CompactTracks } from '../components/CompactTracks';
import { AlbumCard } from './AlbumGrid';
import { usePhone } from '../hooks/usePhone';

const HELP =
  'Try artist:"Name", year:>2000, format:flac, quality:hires, plays:>10, added:<30, -exclude';

/// Instant results across all fields; the advanced syntax works in the same box.
export function SearchView({ query }: { query: string }) {
  const navigate = useStore((s) => s.navigate);
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [found, setResults] = useState<SearchResults | null>(null);
  // An empty box (the phone's Search tab) shows no results.
  const results = query.trim() ? found : null;
  const [error, setError] = useState<string | null>(null);
  // The query that was last saved; the button resets as soon as the query changes.
  const [savedQuery, setSavedQuery] = useState<string | null>(null);
  const saved = savedQuery === query;
  const phone = usePhone();

  useEffect(() => {
    if (!query.trim()) return;
    let live = true;
    // Debounce slightly so fast typing does not queue a search per key.
    const t = setTimeout(() => {
      api.search(query, 200).then(
        (r) => live && (setResults(r), setError(null)),
        (e: unknown) => live && setError(String(e)),
      );
    }, 60);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [query, libraryVersion]);

  const save = async () => {
    const name = window.prompt('Name this search', query);
    if (!name) return;
    const created = await attempt(api.saveSearch(name, query));
    if (created) {
      setSavedQuery(query);
      useStore.getState().libraryChanged();
    }
  };

  return (
    <section className="view scroll search-view">
      {phone && (
        <div className="phone-search">
          <input
            type="search"
            // The Search tab opens ready to type.
            autoFocus={query === ''}
            placeholder="Songs, artists, albums…"
            aria-label="Search"
            value={query}
            onChange={(e) =>
              useStore.setState({ route: { name: 'search', query: e.target.value } })
            }
          />
        </div>
      )}
      {(!phone || query !== '') && (
        <header className="view-header">
          <h1>{query ? `“${query}”` : 'Search'}</h1>
          <div className="view-controls">
            <button className="pill" onClick={() => void save()} disabled={saved || !query}>
              <Bookmark size={14} aria-hidden /> {saved ? 'Saved' : 'Save Search'}
            </button>
          </div>
        </header>
      )}
      {query === '' && <p className="muted search-help">{HELP}</p>}
      {error && (
        <p role="alert" className="muted">
          {error}
        </p>
      )}
      {results &&
        results.songCount === 0 &&
        results.albums.length === 0 &&
        results.artists.length === 0 && (
          <div className="empty-state compact">
            <p>No results.</p>
            <p className="muted">{HELP}</p>
          </div>
        )}

      {results && results.artists.length > 0 && (
        <>
          <h2 className="section-title">Artists</h2>
          <div className="artist-chips">
            {results.artists.map((a) => (
              <button
                key={a.name}
                className="artist-chip"
                onClick={() => navigate({ name: 'artist', artist: a.name })}
              >
                <Artwork hash={a.artHash} size={56} alt="" className="round" />
                <span>{a.name}</span>
              </button>
            ))}
          </div>
        </>
      )}

      {results && results.albums.length > 0 && (
        <>
          <h2 className="section-title">Albums</h2>
          <div className="card-row">
            {results.albums.map((a) => (
              <AlbumCard key={a.id} album={a} size={160} />
            ))}
          </div>
        </>
      )}

      {results && results.songCount > 0 && (
        <>
          <h2 className="section-title">
            Songs{' '}
            <span className="muted">
              {results.songCount.toLocaleString()}
              {results.songCountCapped ? '+' : ''}
            </span>
          </h2>
          <CompactTracks tracks={results.songs} />
          {results.songCount > results.songs.length && (
            <p className="muted">
              Showing the first {results.songs.length}. Narrow the search to see the rest.
            </p>
          )}
        </>
      )}
    </section>
  );
}
