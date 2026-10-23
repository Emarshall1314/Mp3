import { useEffect, useRef, useState } from 'react';
import { GripVertical, ListPlus, Pencil, Play, Plus, Shuffle, Trash2, X } from 'lucide-react';
import { api } from '../api';
import { menuHandler, newPlaylist, trackMenu } from '../actions';
import { attempt, type MenuItem, useStore } from '../store';
import type { Playlist, PlaylistDetail } from '../types';
import { duration } from '../format';
import { Artwork } from '../components/Artwork';
import { Collage } from '../components/Collage';

function usePlaylists(): Playlist[] | null {
  const version = useStore((s) => s.playlistsVersion);
  const libraryVersion = useStore((s) => s.libraryVersion);
  const [lists, setLists] = useState<Playlist[] | null>(null);
  useEffect(() => {
    let live = true;
    void api.playlists().then((l) => live && setLists(l));
    return () => {
      live = false;
    };
  }, [version, libraryVersion]);
  return lists;
}

const changed = () => useStore.getState().playlistsChanged();

function songs(n: number) {
  return `${n} ${n === 1 ? 'song' : 'songs'}`;
}

function renamePlaylist(p: Playlist) {
  useStore
    .getState()
    .askText(
      'Rename Playlist',
      p.name,
      'Rename',
      (name) => void attempt(api.renamePlaylist(p.id, name).then(changed)),
    );
}

/// Deletes a playlist, with Undo (which makes it again with the same songs, in order).
async function deletePlaylist(p: Playlist) {
  const detail = await attempt(api.playlist(p.id));
  if (!detail) return;
  await attempt(api.deletePlaylist(p.id));
  changed();
  const s = useStore.getState();
  if (s.route.name === 'playlist' && s.route.id === p.id) s.navigate({ name: 'playlists' });
  s.showToast(`Deleted “${p.name}”`, {
    label: 'Undo',
    run: () =>
      void attempt(
        api
          .createPlaylist(
            p.name,
            detail.items.map((i) => i.track.id),
          )
          .then(changed),
      ),
  });
}

async function playPlaylist(id: number, shuffle = false, from = 0) {
  const d = await api.playlist(id);
  if (d.items.length)
    await api.playTracks(
      d.items.map((i) => i.track.id),
      from,
      shuffle,
    );
}

function playlistMenu(p: Playlist): MenuItem[] {
  return [
    { label: 'Play', run: () => void attempt(playPlaylist(p.id)) },
    { label: 'Shuffle', run: () => void attempt(playPlaylist(p.id, true)) },
    { label: 'Rename…', run: () => renamePlaylist(p) },
    { label: 'Delete Playlist', run: () => void deletePlaylist(p) },
  ];
}

/// Playlists: a card for each (newest change first), and one for making a new one.
export function PlaylistsView() {
  const lists = usePlaylists();
  const navigate = useStore((s) => s.navigate);
  return (
    <section className="view scroll playlists-view">
      <header className="view-header">
        <h1>Playlists</h1>
      </header>
      <ul className="card-grid playlist-grid" aria-label="Playlists">
        <li>
          <button className="playlist-card new" onClick={() => newPlaylist()}>
            <span className="new-tile" aria-hidden>
              <Plus size={36} />
            </span>
            <span className="title">New Playlist</span>
          </button>
        </li>
        {lists?.map((p) => (
          <li key={p.id} onContextMenu={menuHandler(() => playlistMenu(p))}>
            <button
              className="playlist-card"
              onClick={() => navigate({ name: 'playlist', id: p.id })}
            >
              <Collage art={p.art} size={170} />
              <span className="title">{p.name}</span>
              <span className="subtitle">{songs(p.trackCount)}</span>
            </button>
          </li>
        ))}
      </ul>
      {lists?.length === 0 && (
        <p className="muted home-hint">
          Make one here, or choose Add to Playlist… on any song or album (right-click, or press and
          hold on a phone). A Rediscover mix can be saved as one too.
        </p>
      )}
    </section>
  );
}

/// One playlist: its cover and buttons, and its songs, which can be dragged into a new order
/// (by the grip), removed, or played from any of them.
export function PlaylistView({ id }: { id: number }) {
  const version = useStore((s) => s.playlistsVersion);
  const libraryVersion = useStore((s) => s.libraryVersion);
  const playingId = useStore((s) => s.player?.trackId);
  const [detail, setDetail] = useState<PlaylistDetail | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    api.playlist(id).then(
      (d) => live && setDetail(d),
      () => live && setDetail(null),
    );
    return () => {
      live = false;
    };
  }, [id, version, libraryVersion]);

  // Dragging a row by its grip: which row, how far, and where it would land.
  const [drag, setDrag] = useState<{ from: number; dy: number; row: number } | null>(null);
  const list = useRef<HTMLOListElement>(null);

  if (detail === undefined) return <section className="view" aria-busy="true" />;
  if (detail === null)
    return (
      <section className="view scroll">
        <header className="view-header">
          <h1>Playlist</h1>
        </header>
        <p className="muted home-hint">This playlist has been deleted.</p>
      </section>
    );
  const { playlist: p, items } = detail;
  const ids = items.map((i) => i.track.id);
  const target = drag
    ? Math.max(0, Math.min(items.length - 1, drag.from + Math.round(drag.dy / drag.row)))
    : null;

  const drop = (from: number, to: number) => {
    if (from === to) return;
    const moving = items[from];
    if (!moving) return;
    // Before the item now at `to` when moving up; before the one after it when moving down.
    const before = to < from ? items[to]?.itemId : items[to + 1]?.itemId;
    const reordered = [...items];
    reordered.splice(from, 1);
    reordered.splice(to, 0, moving);
    setDetail({ playlist: p, items: reordered });
    void attempt(api.moveInPlaylist(p.id, moving.itemId, before ?? null).then(changed));
  };

  return (
    <section className="view scroll playlist-page">
      <div className="album-hero">
        <Collage art={p.art} size={220} />
        <div className="album-hero-text">
          <h1>{p.name}</h1>
          <p className="muted">
            {songs(p.trackCount)}
            {p.durationMs > 0 && `, ${Math.round(p.durationMs / 60000)} min`}
          </p>
          <div className="action-row">
            <button
              className="pill primary"
              disabled={!items.length}
              onClick={() => void attempt(api.playTracks(ids, 0))}
            >
              <Play size={14} fill="currentColor" aria-hidden /> Play
            </button>
            <button
              className="pill"
              disabled={!items.length}
              onClick={() => void attempt(api.playTracks(ids, 0, true))}
            >
              <Shuffle size={14} aria-hidden /> Shuffle
            </button>
            <button className="icon-button" aria-label="Rename" onClick={() => renamePlaylist(p)}>
              <Pencil size={16} />
            </button>
            <button
              className="icon-button"
              aria-label="Delete playlist"
              onClick={() => void deletePlaylist(p)}
            >
              <Trash2 size={16} />
            </button>
          </div>
        </div>
      </div>
      {items.length === 0 ? (
        <p className="muted home-hint">
          <ListPlus size={16} aria-hidden /> Add songs with Add to Playlist… on any song or album.
        </p>
      ) : (
        <ol className="playlist-items" ref={list} data-dragging={drag ? '' : undefined}>
          {items.map((it, i) => {
            let shift = 0;
            if (drag && target !== null) {
              if (i === drag.from) shift = drag.dy;
              else if (drag.from < i && i <= target) shift = -drag.row;
              else if (target <= i && i < drag.from) shift = drag.row;
            }
            return (
              <li
                key={it.itemId}
                className={`playlist-row ${it.track.id === playingId ? 'playing' : ''}`}
                data-lifted={drag?.from === i || undefined}
                style={shift ? { transform: `translateY(${shift}px)` } : undefined}
                onContextMenu={menuHandler(() => [
                  ...trackMenu(it.track),
                  {
                    label: 'Remove from Playlist',
                    run: () =>
                      void attempt(api.removeFromPlaylist(p.id, [it.itemId]).then(changed)),
                  },
                ])}
              >
                <button
                  className="playlist-play"
                  onClick={() => void attempt(api.playTracks(ids, i))}
                  aria-label={`Play ${it.track.title}`}
                >
                  <Artwork hash={it.track.artHash} size={44} alt="" />
                  <span className="queue-text">
                    <span className="title">{it.track.title}</span>
                    <span className="subtitle">{it.track.artist}</span>
                  </span>
                  <span className="time">{duration(it.track.durationMs)}</span>
                </button>
                <button
                  className="icon-button small"
                  aria-label={`Remove ${it.track.title}`}
                  onClick={() =>
                    void attempt(api.removeFromPlaylist(p.id, [it.itemId]).then(changed))
                  }
                >
                  <X size={14} />
                </button>
                <span
                  className="playlist-grip"
                  role="button"
                  tabIndex={0}
                  aria-label={`Move ${it.track.title} (arrow keys)`}
                  data-no-swipe=""
                  onKeyDown={(e) => {
                    if (e.key === 'ArrowUp' && i > 0) drop(i, i - 1);
                    else if (e.key === 'ArrowDown' && i < items.length - 1) drop(i, i + 1);
                    else return;
                    e.preventDefault();
                  }}
                  onPointerDown={(e) => {
                    e.preventDefault();
                    e.currentTarget.setPointerCapture(e.pointerId);
                    const row = e.currentTarget.parentElement?.offsetHeight ?? 56;
                    setDrag({ from: i, dy: 0, row });
                    (e.currentTarget as HTMLElement).dataset.y = String(e.clientY);
                  }}
                  onPointerMove={(e) => {
                    const y0 = Number((e.currentTarget as HTMLElement).dataset.y);
                    if (drag && drag.from === i) setDrag({ ...drag, dy: e.clientY - y0 });
                  }}
                  onPointerUp={() => {
                    if (drag && target !== null) drop(drag.from, target);
                    setDrag(null);
                  }}
                  onPointerCancel={() => setDrag(null)}
                >
                  <GripVertical size={18} aria-hidden />
                </span>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
