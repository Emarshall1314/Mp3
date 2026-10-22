import { useEffect, useState } from 'react';
import { FolderSync, Pause, Play, RefreshCw } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { SyncConflict, SyncSettings, SyncStatus } from '../types';
import { pickFolder } from './EmptyLibrary';

const FIELD_NAMES: Record<string, string> = {
  favourite: 'Favourite',
  trash: 'In the Trash',
  pictures: 'Artwork',
  albumartist: 'Album artist',
  tracknumber: 'Track number',
};

const fieldName = (f: string) => FIELD_NAMES[f] ?? f.charAt(0).toUpperCase() + f.slice(1);

/// A value as the owner reads it.
function shown(field: string, values: string[]): string {
  if (field === 'favourite') return values[0] === 'true' ? 'Favourite' : 'Not a favourite';
  if (field === 'trash') return values[0] === 'true' ? 'In the Trash' : 'Not in the Trash';
  if (field === 'pictures') return values.length ? `${values.length} picture(s)` : 'No artwork';
  return values.length ? values.join('; ') : '(empty)';
}

const when = (ms: number) => new Date(ms).toLocaleString();

function size(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${Math.round(bytes / 1e3)} KB`;
}

async function save(status: SyncStatus, patch: Partial<SyncSettings>) {
  const next = await attempt(api.setSyncSettings({ ...status.settings, ...patch }));
  if (next) useStore.getState().setSync(next);
}

/// This device's name as others see it; saved when the field is left.
function DeviceName({ status }: { status: SyncStatus }) {
  const saved = status.settings.deviceName;
  const [name, setName] = useState(saved);
  return (
    <label className="field-row">
      <span>This device’s name</span>
      <input
        value={name}
        maxLength={60}
        onChange={(e) => setName(e.target.value)}
        onBlur={() => name.trim() !== saved && void save(status, { deviceName: name })}
        onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
      />
    </label>
  );
}

/// Sync: a folder the owner's devices share, this device's name and options, what happened,
/// the other devices, and edits that clashed.
export function SyncView() {
  const status = useStore((s) => s.sync);
  const [conflicts, setConflicts] = useState<SyncConflict[]>([]);
  const deviceName = status?.settings.deviceName ?? '';
  const conflictCount = status?.conflicts ?? 0;
  const lastSync = status?.lastSyncMs ?? null;

  useEffect(() => {
    void api.syncConflicts().then(setConflicts);
  }, [conflictCount, lastSync]);

  if (!status) return null;
  const on = status.settings.folder !== null;
  const t = status.transfer;

  return (
    <section className="view scroll sync-view">
      <header className="view-header">
        <h1>Sync</h1>
        <div className="view-controls">
          {on && (
            <button
              className="pill"
              onClick={() => void save(status, { paused: !status.settings.paused })}
            >
              {status.settings.paused ? (
                <>
                  <Play size={14} aria-hidden /> Resume
                </>
              ) : (
                <>
                  <Pause size={14} aria-hidden /> Pause
                </>
              )}
            </button>
          )}
          <button
            className="pill primary"
            disabled={!on || status.settings.paused || status.syncing}
            onClick={() => void attempt(api.syncNow())}
          >
            <RefreshCw size={14} aria-hidden /> Sync Now
          </button>
        </div>
      </header>
      <p className="muted">
        Keeps tag edits, artwork, favourites, play counts and the Trash the same on all your
        devices, through a folder they share: one kept in step by Syncthing, Nextcloud or similar,
        or on a drive you move between them. Point Cadenza on each device at the same folder.
        There’s no server and no account; nothing leaves your devices except through that folder.
      </p>

      <h2 className="section-title">Sync Folder</h2>
      <div className="sync-folder">
        <span className="path">{status.settings.folder ?? 'None: sync is off'}</span>
        <button
          className="pill"
          onClick={() =>
            void pickFolder('Choose the Sync Folder').then((folder) => {
              if (folder) void save(status, { folder, paused: false });
            })
          }
        >
          <FolderSync size={14} aria-hidden /> {on ? 'Change…' : 'Choose Folder…'}
        </button>
        {on && (
          <button className="pill" onClick={() => void save(status, { folder: null })}>
            Stop Syncing
          </button>
        )}
      </div>
      {on && (
        <p className="muted small">
          Stopping leaves the folder and your music as they are; choose the folder again to carry
          on.
        </p>
      )}

      <DeviceName key={deviceName} status={status} />
      <label className="check-row">
        <input
          type="checkbox"
          checked={status.settings.shareAudio}
          onChange={(e) => void save(status, { shareAudio: e.target.checked })}
        />
        Copy this device’s songs into the sync folder, for devices that don’t have them (needs as
        much space there as your music takes)
      </label>
      <label className="check-row">
        <input
          type="checkbox"
          checked={status.settings.downloadAudio}
          onChange={(e) => void save(status, { downloadAudio: e.target.checked })}
        />
        Copy songs other devices put in the sync folder into my first music folder
      </label>

      {on && (
        <>
          <h2 className="section-title">Status</h2>
          <p role="status">
            {status.settings.paused
              ? 'Paused.'
              : status.syncing
                ? 'Syncing…'
                : lastSync
                  ? `Last synced ${when(lastSync)}.`
                  : 'Not synced yet.'}{' '}
            {status.pending > 0 &&
              `${status.pending} ${status.pending === 1 ? 'change' : 'changes'} waiting to go.`}
          </p>
          {status.error && <p className="error-text">{status.error}</p>}
          {t && (
            <div className="sync-transfer">
              <span>
                {t.direction === 'upload' ? 'Copying to the folder' : 'Copying from the folder'}:{' '}
                {t.name} ({size(t.done)} of {size(t.total)})
                {t.remaining > 0 && `, ${t.remaining} more after this`}
              </span>
              <progress value={t.done} max={Math.max(1, t.total)} />
            </div>
          )}
          {status.problems.length > 0 && (
            <details className="sync-problems">
              <summary>
                {status.problems.length} {status.problems.length === 1 ? 'thing' : 'things'}{' '}
                couldn’t be synced
              </summary>
              <ul>
                {status.problems.map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            </details>
          )}

          <h2 className="section-title">Devices</h2>
          {status.devices.length === 0 ? (
            <p className="muted">Devices appear here after their first sync.</p>
          ) : (
            <ul className="history-list">
              {status.devices.map((d) => (
                <li key={d.id}>
                  <div>
                    <span>
                      {d.name}
                      {d.thisDevice && ' (this device)'}
                    </span>
                    <span className="muted small">
                      {d.platform === 'android'
                        ? 'Android'
                        : d.platform === 'linux'
                          ? 'Linux'
                          : d.platform}{' '}
                      · last synced {when(d.lastPushMs)}
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          )}

          <h2 className="section-title">Conflicts</h2>
          {conflicts.length === 0 ? (
            <p className="muted">
              When two devices change the same thing before syncing, the later change is kept and
              the other is listed here, so nothing is lost.
            </p>
          ) : (
            <ul className="conflict-list">
              {conflicts.map((c) => (
                <li key={`${c.audioHash}/${c.field}/${c.lostAtMs}`}>
                  <p>
                    <strong>{c.title ?? 'A song not on this device'}</strong>
                    {c.artist && ` — ${c.artist}`}: {fieldName(c.field)}
                  </p>
                  <div className="conflict-choices">
                    <button
                      className="pill primary"
                      onClick={() =>
                        void attempt(api.resolveConflict(c.audioHash, c.field, c.kept))
                      }
                    >
                      Keep “{shown(c.field, c.kept)}”
                    </button>
                    <span className="muted small">
                      {c.keptDevice}, {when(c.keptAtMs)} (in use now)
                    </span>
                    <button
                      className="pill"
                      onClick={() =>
                        void attempt(api.resolveConflict(c.audioHash, c.field, c.lost))
                      }
                    >
                      Use “{shown(c.field, c.lost)}”
                    </button>
                    <span className="muted small">
                      {c.lostDevice}, {when(c.lostAtMs)}
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}
