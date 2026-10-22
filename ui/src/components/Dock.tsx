import { useEffect, useState } from 'react';
import { PanelsTopLeft, X } from 'lucide-react';
import { api } from '../api';
import { trackSpec } from '../format';
import { useCurrentTrack } from '../hooks/useCurrentTrack';
import { updateLayout, useLook } from '../hooks/useAppearance';
import { type DockTab, useStore } from '../store';
import type { Dock as DockLayout } from '../theme/layout';
import type { TrackTags } from '../types';
import { Artwork } from './Artwork';
import { LyricsView } from './LyricsView';
import { QueuePanel } from './QueuePanel';
import { SignalPathPanel } from './SignalPath';
import { DockedSongInfo } from './SongInfo';

const TABS: { tab: DockTab; label: string }[] = [
  { tab: 'queue', label: 'Queue' },
  { tab: 'lyrics', label: 'Lyrics' },
  { tab: 'info', label: 'Info' },
  { tab: 'signal', label: 'Signal' },
];

const POSITIONS: [DockLayout['position'], string][] = [
  ['right', 'Dock Right'],
  ['left', 'Dock Left'],
  ['bottom', 'Dock at Bottom'],
  ['floating', 'Float'],
];

/// Credits shown in Info, in this order, when the file has them.
const CREDITS: [string, string][] = [
  ['composer', 'Composer'],
  ['lyricist', 'Lyricist'],
  ['conductor', 'Conductor'],
  ['performer', 'Performers'],
  ['arranger', 'Arranger'],
  ['producer', 'Producer'],
  ['engineer', 'Engineer'],
  ['mixer', 'Mixed by'],
  ['label', 'Label'],
  ['catalognumber', 'Catalogue number'],
  ['date', 'Date'],
  ['originaldate', 'Original date'],
  ['genre', 'Genre'],
  ['isrc', 'ISRC'],
];

/// The playing song's details and credits.
function InfoPanel() {
  const track = useCurrentTrack();
  const [tags, setTags] = useState<TrackTags | null>(null);
  const id = track?.id ?? null;

  useEffect(() => {
    if (id === null) return;
    let live = true;
    api.trackTags([id]).then(
      (t) => live && setTags(t[0] ?? null),
      () => live && setTags(null),
    );
    return () => {
      live = false;
    };
  }, [id]);

  if (!track) return <p className="empty">Nothing is playing.</p>;
  const fields = tags?.trackId === track.id ? tags.fields : {};
  return (
    <div className="info-panel">
      <Artwork hash={track.artHash} size={160} alt={`${track.album} artwork`} />
      <h3>{track.title}</h3>
      <p className="muted">{[track.artist, track.album].filter(Boolean).join(' — ')}</p>
      <dl>
        {CREDITS.filter(([f]) => fields[f]?.length).map(([f, label]) => (
          <div key={f}>
            <dt>{label}</dt>
            <dd>{fields[f]?.join(', ')}</dd>
          </div>
        ))}
        <div>
          <dt>Format</dt>
          <dd>{trackSpec(track)}</dd>
        </div>
        {tags?.path && (
          <div>
            <dt>File</dt>
            <dd className="path">{tags.path}</dd>
          </div>
        )}
      </dl>
    </div>
  );
}

/// Pointer drags reported as distance moved; the layout is saved shortly after.
function drag(e: React.PointerEvent, onMove: (dx: number, dy: number) => void) {
  e.preventDefault();
  const target = e.currentTarget as HTMLElement;
  target.setPointerCapture(e.pointerId);
  const [x0, y0] = [e.clientX, e.clientY];
  const move = (m: PointerEvent) => onMove(m.clientX - x0, m.clientY - y0);
  const up = () => {
    target.removeEventListener('pointermove', move);
    target.removeEventListener('pointerup', up);
  };
  target.addEventListener('pointermove', move);
  target.addEventListener('pointerup', up);
}

const setDock = (patch: Partial<DockLayout>) =>
  updateLayout({ dock: { ...useLook.getState().look.layout.dock, ...patch } });

/// The dock: queue, lyrics, info and signal path in tabs, on the left, right or bottom of the
/// page, or floating over it; resized from its inner edge (or corner when floating).
export function Dock() {
  const tab = useStore((s) => s.dock);
  const setTab = useStore((s) => s.setDock);
  const dock = useLook((s) => s.look.layout.dock);
  const trackId = useStore((s) => s.player?.trackId ?? null);
  const songInfo = useStore((s) => s.songInfo);
  const tagsDocked = useLook((s) => s.look.layout.tagEditorDocked);

  // Get Info… opens in the panel when the tag editor is docked.
  useEffect(() => {
    if (songInfo && tagsDocked) setTab('tags');
  }, [songInfo, tagsDocked, setTab]);
  useEffect(() => {
    if (tab === 'tags' && !tagsDocked) setTab(songInfo ? null : 'queue');
  }, [tab, tagsDocked, songInfo, setTab]);

  if (!tab) return null;
  const tabs = tagsDocked ? [...TABS, { tab: 'tags' as DockTab, label: 'Tags' }] : TABS;

  const floating = dock.position === 'floating';
  const style: React.CSSProperties = floating
    ? { left: dock.x, top: dock.y, width: dock.w, height: dock.h }
    : dock.position === 'bottom'
      ? { height: dock.size }
      : { width: dock.size };

  const resizeEdge = (e: React.PointerEvent) => {
    const start = dock.size;
    drag(e, (dx, dy) => {
      const by = dock.position === 'right' ? -dx : dock.position === 'left' ? dx : -dy;
      setDock({ size: Math.min(800, Math.max(200, start + by)) });
    });
  };

  return (
    <aside className="dock" data-position={dock.position} style={style} aria-label="Panel">
      <header
        className="dock-header"
        onPointerDown={
          floating
            ? (e) => {
                if ((e.target as HTMLElement).closest('button')) return;
                const [x, y] = [dock.x, dock.y];
                drag(e, (dx, dy) => setDock({ x: Math.max(0, x + dx), y: Math.max(0, y + dy) }));
              }
            : undefined
        }
      >
        <div className="tabs" role="tablist">
          {tabs.map((t) => (
            <button
              key={t.tab}
              role="tab"
              aria-selected={tab === t.tab}
              className={tab === t.tab ? 'on' : ''}
              onClick={() => setTab(t.tab)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <button
          className="icon-button small"
          aria-label="Move panel"
          title="Move panel"
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            useStore.getState().openMenu(
              r.left,
              r.bottom + 4,
              POSITIONS.map(([position, label]) => ({
                label: position === dock.position ? `${label} ✓` : label,
                run: () => setDock({ position }),
              })),
            );
          }}
        >
          <PanelsTopLeft size={14} />
        </button>
        <button className="icon-button small" aria-label="Close panel" onClick={() => setTab(null)}>
          <X size={14} />
        </button>
      </header>
      <div className="dock-body" role="tabpanel">
        {tab === 'queue' && <QueuePanel />}
        {tab === 'lyrics' && <LyricsView trackId={trackId} />}
        {tab === 'info' && <InfoPanel />}
        {tab === 'signal' && <SignalPathPanel />}
        {tab === 'tags' && <DockedSongInfo />}
      </div>
      {floating ? (
        <span
          className="dock-corner"
          aria-hidden
          onPointerDown={(e) => {
            const [w, h] = [dock.w, dock.h];
            drag(e, (dx, dy) => setDock({ w: Math.max(240, w + dx), h: Math.max(200, h + dy) }));
          }}
        />
      ) : (
        <span className="dock-edge" aria-hidden onPointerDown={resizeEdge} />
      )}
    </aside>
  );
}
