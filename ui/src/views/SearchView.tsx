import { useEffect, useState } from 'react';
import { Bookmark } from 'lucide-react';
import { api } from '../api';
import { menuHandler, trackMenu } from '../actions';
import { duration } from '../format';
import { attempt, useStore } from '../store';
import type { SearchResults } from '../types';
import { Artwork } from '../components/Artwork';
import { AlbumCard } from './AlbumGrid';

const HELP =
  'Try artist:"Name", year:>2000, format:flac, quality:hires, rating:>=4, added:<30, -exclude';

/// Instant results across all fields; the advanced syntax works in the same box.
export function SearchView({ query }: { query: string }) {
  const navigate = useStore((s) => s.navigate);
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [results, setResults] = useState<SearchResults | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The query that was last saved; the button resets as soon as the query changes.
  const [savedQuery, setSavedQuery] = useState<string | null>(null);
  const saved = savedQuery === query;

  useEffect(() => {
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
      <header className="view-header">
        <h1>“{query}”</h1>
        <div className="view-controls">
          <button className="pill" onClick={() => void save()} disabled={saved}>
            <Bookmark size={14} aria-hidden /> {saved ? 'Saved' : 'Save Search'}
          </button>
        </div>
      </header>
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
          <div className="result-songs" role="list">
            {results.songs.map((t, i) => (
              <div
                key={t.id}
                role="listitem"
                className="result-song"
                onDoubleClick={() =>
                  void attempt(
                    api.playTracks(
                      results.songs.map((s) => s.id),
                      i,
                    ),
                  )
                }
                onContextMenu={menuHandler(() => trackMenu(t))}
              >
                <button
                  className="art-button"
                  aria-label={`Play ${t.title}`}
                  onClick={() =>
                    void attempt(
                      api.playTracks(
                        results.songs.map((s) => s.id),
                        i,
                      ),
                    )
                  }
                >
                  <Artwork hash={t.artHash} size={40} alt="" />
                </button>
                <div className="queue-text">
                  <span className="title">{t.title}</span>
                  <span className="subtitle">
                    {t.artist} — {t.album}
                  </span>
                </div>
                <span className="time">{duration(t.durationMs)}</span>
              </div>
            ))}
          </div>
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
