import { useEffect, useState } from 'react';
import { FolderSync, Pause, Play, RefreshCw } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { SyncConflict, SyncFolderInfo, SyncSettings, SyncStatus } from '../types';
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

async function save(
  status: SyncStatus,
  patch: Partial<SyncSettings>,
  passphrase: string | null = null,
): Promise<boolean> {
  const next = await attempt(api.setSyncSettings({ ...status.settings, ...patch }, passphrase));
  if (next) useStore.getState().setSync(next);
  return !!next;
}

/// A folder about to be used: what it holds decides what's asked. A new folder can be encrypted
/// (passphrase typed twice); an encrypted one needs its passphrase; an unencrypted one just joins.
/// Also unlocks the current folder when this device has lost its key.
function FolderSetup({
  status,
  folder,
  info,
  onDone,
  locked = false,
}: {
  status: SyncStatus;
  folder: string;
  info: SyncFolderInfo;
  onDone: () => void;
  /** The current folder, whose key this device doesn't have. */
  locked?: boolean;
}) {
  const [encrypt, setEncrypt] = useState(false);
  const [pass, setPass] = useState('');
  const [again, setAgain] = useState('');
  const [busy, setBusy] = useState(false);
  const isNew = !info.hasLibrary;
  const needsPass = info.encrypted || (isNew && encrypt);
  const mismatch = isNew && encrypt && again !== '' && pass !== again;
  const ready = !needsPass || (pass.length >= 8 && (!isNew || pass === again));
  const go = async () => {
    setBusy(true);
    const ok = await save(status, { folder, paused: false }, needsPass ? pass : null);
    setBusy(false);
    if (ok) onDone();
  };
  return (
    <form
      className="folder-setup"
      onSubmit={(e) => {
        e.preventDefault();
        if (ready && !busy) void go();
      }}
    >
      {!locked && <p className="path">{folder}</p>}
      <p>
        {locked
          ? 'The sync folder is encrypted and this device doesn’t have its key. Enter the passphrase to carry on syncing.'
          : isNew
            ? 'This folder has no Cadenza library yet: this device will start one.'
            : info.encrypted
              ? `This folder holds an encrypted Cadenza library${info.devices ? ` shared by ${info.devices} ${info.devices === 1 ? 'device' : 'devices'}` : ''}. Enter its passphrase.`
              : `This folder holds a Cadenza library${info.devices ? ` shared by ${info.devices} ${info.devices === 1 ? 'device' : 'devices'}` : ''}.`}
      </p>
      {isNew && (
        <label className="check-row">
          <input type="checkbox" checked={encrypt} onChange={(e) => setEncrypt(e.target.checked)} />
          Encrypt it with a passphrase, so whoever holds the folder can’t read it
        </label>
      )}
      {needsPass && (
        <>
          <label className="field-row">
            <span>Passphrase</span>
            <input
              type="password"
              autoComplete={isNew ? 'new-password' : 'current-password'}
              value={pass}
              onChange={(e) => setPass(e.target.value)}
            />
          </label>
          {isNew && (
            <label className="field-row">
              <span>Again</span>
              <input
                type="password"
                autoComplete="new-password"
                value={again}
                onChange={(e) => setAgain(e.target.value)}
              />
            </label>
          )}
          <p className="muted small" role={mismatch ? 'alert' : undefined}>
            {mismatch
              ? 'The two passphrases differ.'
              : isNew
                ? 'At least 8 characters. Every device needs it to join; there’s no way to recover it, so keep it somewhere safe.'
                : 'The passphrase this library was encrypted with.'}
          </p>
        </>
      )}
      <div className="row-actions">
        <button className="pill primary" type="submit" disabled={!ready || busy}>
          {busy ? 'Working…' : locked ? 'Unlock' : isNew ? 'Start Syncing' : 'Join'}
        </button>
        {!locked && (
          <button className="pill" type="button" onClick={onDone}>
            Cancel
          </button>
        )}
      </div>
    </form>
  );
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
  // A folder being set up (chosen, or the current one locked), and what it holds.
  const [setup, setSetup] = useState<{ folder: string; info: SyncFolderInfo } | null>(null);
  const [locked, setLocked] = useState(false);
  const folderNow = status?.settings.folder ?? null;
  const encryptedHere = status?.encrypted ?? false;
  useEffect(() => {
    if (!folderNow || encryptedHere) return;
    let live = true;
    void api.syncFolderInfo(folderNow).then(
      (info) => live && setLocked(info.encrypted),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [folderNow, encryptedHere]);
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
        <span className="path">
          {status.settings.folder ?? 'None: sync is off'}
          {on && status.encrypted && <span className="quality-badge">Encrypted</span>}
        </span>
        <button
          className="pill"
          onClick={() =>
            void pickFolder('Choose the Sync Folder').then(async (folder) => {
              if (!folder) return;
              const info = await attempt(api.syncFolderInfo(folder));
              if (info) setSetup({ folder, info });
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
      {setup && (
        <FolderSetup
          status={status}
          folder={setup.folder}
          info={setup.info}
          onDone={() => {
            setSetup(null);
            setLocked(false);
          }}
        />
      )}
      {on && locked && !setup && folderNow && (
        <FolderSetup
          status={status}
          folder={folderNow}
          info={{ hasLibrary: true, encrypted: true, devices: 0 }}
          onDone={() => setLocked(false)}
          locked
        />
      )}
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
