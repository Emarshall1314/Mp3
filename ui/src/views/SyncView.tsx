import { useEffect, useState } from 'react';
import { FolderSync, Pause, Play, RefreshCw, Server } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { Remote, SyncConflict, SyncFolderInfo, SyncSettings, SyncStatus } from '../types';
import { pickFolder } from './EmptyLibrary';
import { duration } from '../format';

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
  password: string | null = null,
): Promise<boolean> {
  const next = await attempt(
    api.setSyncSettings({ ...status.settings, ...patch }, passphrase, password),
  );
  if (next) useStore.getState().setSync(next);
  return !!next;
}

/// How a server shows in the page.
export function describeRemote(r: Remote): string {
  if (r.kind === 'webDav') return `${r.username ? `${r.username} at ` : ''}${r.url}`;
  const where = r.endpoint || 'AWS S3';
  return `${r.bucket}${r.root ? `/${r.root}` : ''} on ${where}`;
}

/// A place to sync through, chosen but not yet in use.
interface Setup {
  /** "folder" or "server", for the wording. */
  kind: 'folder' | 'server';
  label: string;
  patch: Partial<SyncSettings>;
  password: string | null;
  info: SyncFolderInfo;
}

/// A folder or server about to be used: what it holds decides what's asked. A new one can be
/// encrypted (passphrase typed twice); an encrypted one needs its passphrase; an unencrypted one
/// just joins. Also unlocks the current one when this device has lost its key.
function PlaceSetup({
  status,
  setup,
  onDone,
  locked = false,
}: {
  status: SyncStatus;
  setup: Setup;
  onDone: () => void;
  /** The place in use, whose key this device doesn't have. */
  locked?: boolean;
}) {
  const { kind, label, patch, password, info } = setup;
  const place = kind === 'folder' ? 'folder' : 'server folder';
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
    const ok = await save(status, { ...patch, paused: false }, needsPass ? pass : null, password);
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
      {!locked && <p className="path">{label}</p>}
      <p>
        {locked
          ? 'The sync library is encrypted and this device doesn’t have its key. Enter the passphrase to carry on syncing.'
          : isNew
            ? `This ${place} has no Cadenza library yet: this device will start one.`
            : info.encrypted
              ? `This ${place} holds an encrypted Cadenza library${info.devices ? ` shared by ${info.devices} ${info.devices === 1 ? 'device' : 'devices'}` : ''}. Enter its passphrase.`
              : `This ${place} holds a Cadenza library${info.devices ? ` shared by ${info.devices} ${info.devices === 1 ? 'device' : 'devices'}` : ''}.`}
      </p>
      {isNew && (
        <label className="check-row">
          <input type="checkbox" checked={encrypt} onChange={(e) => setEncrypt(e.target.checked)} />
          Encrypt it with a passphrase, so whoever holds the {place} can’t read it
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

const EMPTY_WEBDAV: Remote = { kind: 'webDav', url: '', username: '' };
const EMPTY_S3: Remote = {
  kind: 's3',
  endpoint: '',
  bucket: '',
  region: '',
  accessKeyId: '',
  root: 'Cadenza',
};

/// A server to sync through: WebDAV (Nextcloud and the like) or S3-compatible storage. Checking
/// it connects and looks at what's there; then the usual setup (encryption) follows.
function ServerForm({
  initial,
  onChecked,
  onCancel,
}: {
  initial: Remote | null;
  onChecked: (setup: Setup) => void;
  onCancel: () => void;
}) {
  const [remote, setRemote] = useState<Remote>(initial ?? EMPTY_WEBDAV);
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<Record<string, string>>) =>
    setRemote((r) => ({ ...r, ...patch }) as Remote);
  const ready =
    password !== '' &&
    (remote.kind === 'webDav'
      ? /^https?:\/\/./.test(remote.url)
      : remote.bucket !== '' && remote.accessKeyId !== '');
  const field = (label: string, key: string, value: string, hint?: string) => (
    <label className="field-row" key={key}>
      <span>{label}</span>
      <input
        value={value}
        placeholder={hint}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => set({ [key]: e.target.value })}
      />
    </label>
  );
  return (
    <form
      className="folder-setup"
      onSubmit={(e) => {
        e.preventDefault();
        if (!ready || busy) return;
        setBusy(true);
        void attempt(api.syncRemoteInfo(remote, password)).then((info) => {
          setBusy(false);
          if (info)
            onChecked({
              kind: 'server',
              label: describeRemote(remote),
              patch: { remote, folder: null },
              password,
              info,
            });
        });
      }}
    >
      <label className="field-row">
        <span>Kind</span>
        <select
          value={remote.kind}
          onChange={(e) => setRemote(e.target.value === 's3' ? EMPTY_S3 : EMPTY_WEBDAV)}
        >
          <option value="webDav">WebDAV (Nextcloud, ownCloud, a NAS…)</option>
          <option value="s3">S3-compatible (Backblaze B2, AWS, MinIO…)</option>
        </select>
      </label>
      {remote.kind === 'webDav' ? (
        <>
          {field(
            'Address',
            'url',
            remote.url,
            'https://cloud.example.com/remote.php/dav/files/me/Cadenza',
          )}
          {field('User name', 'username', remote.username)}
        </>
      ) : (
        <>
          {field('Endpoint', 'endpoint', remote.endpoint, 'empty for AWS')}
          {field('Bucket', 'bucket', remote.bucket)}
          {field('Region', 'region', remote.region, 'us-east-1')}
          {field('Folder in bucket', 'root', remote.root)}
          {field('Access key', 'accessKeyId', remote.accessKeyId)}
        </>
      )}
      <label className="field-row">
        <span>{remote.kind === 'webDav' ? 'Password' : 'Secret key'}</span>
        <input
          type="password"
          autoComplete="off"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
      </label>
      <p className="muted small">
        {remote.kind === 'webDav'
          ? 'For Nextcloud, use an app password (Settings → Security). The address is your WebDAV address with a folder for Cadenza at the end.'
          : 'Use a key that can only reach this bucket.'}{' '}
        The password stays on this device, in Cadenza’s data folder.
      </p>
      <div className="row-actions">
        <button className="pill primary" type="submit" disabled={!ready || busy}>
          {busy ? 'Connecting…' : 'Connect'}
        </button>
        <button className="pill" type="button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/// A cap on song transfers to and from a server; empty for none.
function Bandwidth({ status }: { status: SyncStatus }) {
  const saved = status.settings.bandwidthKbps;
  const [value, setValue] = useState(saved ? String(saved) : '');
  const commit = () => {
    const n = Math.round(Number(value));
    const next = value.trim() === '' || !Number.isFinite(n) || n <= 0 ? null : n;
    if (next !== saved) void save(status, { bandwidthKbps: next });
  };
  return (
    <label className="field-row">
      <span>Limit song transfers to</span>
      <input
        inputMode="numeric"
        value={value}
        placeholder="no limit"
        onChange={(e) => setValue(e.target.value.replace(/[^0-9]/g, ''))}
        onBlur={commit}
        onKeyDown={(e) => e.key === 'Enter' && (e.target as HTMLInputElement).blur()}
      />
      <span>KB/s</span>
    </label>
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
  // A place being set up (chosen, not yet in use) and what it holds; the server form.
  const [setup, setSetup] = useState<Setup | null>(null);
  const [serverForm, setServerForm] = useState(false);
  const deviceName = status?.settings.deviceName ?? '';
  const conflictCount = status?.conflicts ?? 0;
  const lastSync = status?.lastSyncMs ?? null;

  useEffect(() => {
    void api.syncConflicts().then(setConflicts);
  }, [conflictCount, lastSync]);

  if (!status) return null;
  const on = status.settings.folder !== null || status.settings.remote !== null;
  const remote = status.settings.remote;
  const where = remote
    ? describeRemote(remote)
    : (status.settings.folder ?? 'Nowhere yet: sync is off');
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
        devices, through a place they all reach: a folder they share (kept in step by Syncthing or
        similar, or on a drive you move between them) or a server of yours (WebDAV such as
        Nextcloud, or S3-compatible storage). Point Cadenza on each device at the same place.
        There’s no Cadenza account; nothing leaves your devices except to that place.
      </p>

      <h2 className="section-title">Where</h2>
      <div className="sync-folder">
        <span className="path">
          {where}
          {on && status.encrypted && <span className="quality-badge">Encrypted</span>}
        </span>
        <button
          className="pill"
          onClick={() =>
            void pickFolder('Choose the Sync Folder').then(async (folder) => {
              if (!folder) return;
              const info = await attempt(api.syncFolderInfo(folder));
              if (info) {
                setServerForm(false);
                setSetup({
                  kind: 'folder',
                  label: folder,
                  patch: { folder, remote: null },
                  password: null,
                  info,
                });
              }
            })
          }
        >
          <FolderSync size={14} aria-hidden /> Choose Folder…
        </button>
        <button
          className="pill"
          onClick={() => {
            setSetup(null);
            setServerForm(true);
          }}
        >
          <Server size={14} aria-hidden /> Use a Server…
        </button>
        {on && (
          <button
            className="pill"
            onClick={() => void save(status, { folder: null, remote: null })}
          >
            Stop Syncing
          </button>
        )}
      </div>
      {serverForm && !setup && (
        <ServerForm
          initial={remote}
          onChecked={(s) => {
            setServerForm(false);
            setSetup(s);
          }}
          onCancel={() => setServerForm(false)}
        />
      )}
      {setup && <PlaceSetup status={status} setup={setup} onDone={() => setSetup(null)} />}
      {on && status.locked && !setup && !serverForm && (
        <PlaceSetup
          status={status}
          setup={{
            kind: remote ? 'server' : 'folder',
            label: where,
            patch: {},
            password: null,
            info: { hasLibrary: true, encrypted: true, devices: 0 },
          }}
          onDone={() => undefined}
          locked
        />
      )}
      {on && (
        <p className="muted small">
          Stopping leaves the sync library and your music as they are; choose it again to carry on.
        </p>
      )}

      <DeviceName key={deviceName} status={status} />
      <label className="check-row">
        <input
          type="checkbox"
          checked={status.settings.shareAudio}
          onChange={(e) => void save(status, { shareAudio: e.target.checked })}
        />
        Copy this device’s songs to where you sync, for devices that don’t have them (needs as much
        space there as your music takes)
      </label>
      <label className="check-row">
        <input
          type="checkbox"
          checked={status.settings.downloadAudio}
          onChange={(e) => void save(status, { downloadAudio: e.target.checked })}
        />
        Copy songs other devices put there into my first music folder
      </label>
      {remote && <Bandwidth key={status.settings.bandwidthKbps ?? 0} status={status} />}

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

          {status.handoffs.length > 0 && (
            <>
              <h2 className="section-title">Continue Listening</h2>
              <ul className="history-list">
                {status.handoffs.map((h) => (
                  <li key={h.deviceId}>
                    <div>
                      <span>
                        {h.title || 'A song'}
                        {h.artist && ` — ${h.artist}`}
                      </span>
                      <span className="muted small">
                        {h.deviceName}, {when(h.atMs)} · at {duration(h.positionMs)}
                        {h.here < h.songs &&
                          ` · ${h.here} of ${h.songs} songs in that queue are on this device`}
                      </span>
                    </div>
                    <button
                      className="pill primary"
                      disabled={h.here === 0}
                      title={h.here === 0 ? 'None of those songs are on this device' : undefined}
                      onClick={() => void attempt(api.continueFrom(h.deviceId))}
                    >
                      Continue Here
                    </button>
                  </li>
                ))}
              </ul>
            </>
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
