import { useEffect, useState } from 'react';
import { FolderPlus, RefreshCw, Trash2 } from 'lucide-react';
import { api } from '../api';
import { longDuration } from '../format';
import { attempt, useStore } from '../store';
import type { EditBatch, LibraryStats, Root } from '../types';
import { addFolder } from './EmptyLibrary';

/// Music folders: add, remove, rescan, and what the last scan found.
export function FoldersView() {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const scan = useStore((s) => s.scan);
  const [roots, setRoots] = useState<Root[]>([]);
  const [stats, setStats] = useState<LibraryStats | null>(null);

  useEffect(() => {
    void api.roots().then(setRoots);
    void api.stats().then(setStats);
  }, [libraryVersion, scan.running]);

  const report = scan.lastReport;
  return (
    <section className="view scroll folders-view">
      <header className="view-header">
        <h1>Music Folders</h1>
        <div className="view-controls">
          <button
            className="pill"
            onClick={() => void attempt(api.rescan())}
            disabled={scan.running || roots.length === 0}
          >
            <RefreshCw size={14} aria-hidden /> Rescan
          </button>
          <button className="pill primary" onClick={() => void addFolder()}>
            <FolderPlus size={14} aria-hidden /> Add Folder…
          </button>
        </div>
      </header>
      <p className="muted">
        Cadenza reads these folders, and changes a file only when you edit its tags. Changes on disk
        are picked up automatically.
      </p>
      <ul className="folder-list">
        {roots.map((r) => (
          <li key={r.id}>
            <span className="path">{r.path}</span>
            <button
              className="icon-button small"
              aria-label={`Remove ${r.path} from the library`}
              title="Remove from library (files are not touched)"
              onClick={() =>
                void attempt(api.removeRoot(r.id)).then(() => useStore.getState().libraryChanged())
              }
            >
              <Trash2 size={14} />
            </button>
          </li>
        ))}
      </ul>
      {stats && (
        <p>
          {stats.tracks.toLocaleString()} songs · {stats.albums.toLocaleString()} albums ·{' '}
          {stats.artists.toLocaleString()} artists · {longDuration(stats.durationMs)}
        </p>
      )}
      {scan.running && scan.progress && (
        <p role="status">
          Scanning: {scan.progress.read.toLocaleString()} of {scan.progress.toRead.toLocaleString()}{' '}
          new or changed files read ({scan.progress.found.toLocaleString()} files found)
        </p>
      )}
      {report && !scan.running && (
        <div className="scan-report">
          <h2 className="section-title">Last scan</h2>
          <p>
            {report.added} added · {report.updated} updated · {report.moved} moved ·{' '}
            {report.missing} missing · {report.unchanged} unchanged ·{' '}
            {(report.elapsedMs / 1000).toFixed(1)} s
          </p>
          {report.failed > 0 && (
            <details>
              <summary>{report.failed} files could not be read</summary>
              <ul className="scan-errors">
                {report.errors.map(([path, msg]) => (
                  <li key={path + msg}>
                    <span className="path">{path}</span> — {msg}
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}
      <TagChanges />
    </section>
  );
}

/// Recent tag edits, each with Undo, and the backup setting.
function TagChanges() {
  const libraryVersion = useStore((s) => s.libraryVersion);
  const editing = useStore((s) => s.edit?.running ?? false);
  const [batches, setBatches] = useState<EditBatch[]>([]);
  const [backups, setBackups] = useState<boolean | null>(null);
  const [ratings, setRatings] = useState<boolean | null>(null);

  useEffect(() => {
    void api.editHistory(20).then(setBatches);
  }, [libraryVersion, editing]);
  useEffect(() => {
    void api.settings().then((s) => {
      setBackups(s.tagBackups);
      setRatings(s.ratingsToFiles);
    });
  }, []);

  return (
    <div className="tag-changes">
      <h2 className="section-title">Tag Changes</h2>
      <label className="check-row">
        <input
          type="checkbox"
          checked={backups ?? false}
          disabled={backups === null}
          onChange={(e) => {
            setBackups(e.target.checked);
            void attempt(api.setTagBackups(e.target.checked));
          }}
        />
        Keep a copy of each file before changing its tags (in Cadenza’s data folder)
      </label>
      <label className="check-row">
        <input
          type="checkbox"
          checked={ratings ?? false}
          disabled={ratings === null}
          onChange={(e) => {
            setRatings(e.target.checked);
            void attempt(api.setRatingsToFiles(e.target.checked));
          }}
        />
        Write ratings into the files too, so other players see them (otherwise they stay in Cadenza)
      </label>
      {batches.length === 0 ? (
        <p className="muted">Tag edits appear here, with a way to undo them.</p>
      ) : (
        <ul className="history-list">
          {batches.map((b) => (
            <li key={b.id}>
              <div>
                <span>{b.description}</span>
                <span className="muted small">
                  {new Date(b.at * 1000).toLocaleString()} · {b.tracks}{' '}
                  {b.tracks === 1 ? 'song' : 'songs'}
                  {b.failed > 0 && ` · ${b.failed} not changed`}
                  {b.undone && ' · undone'}
                </span>
              </div>
              <button
                className="pill"
                disabled={b.undone || b.tracks === 0 || !b.finished || editing}
                onClick={() => void attempt(api.undoEdit(b.id))}
              >
                Undo
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
