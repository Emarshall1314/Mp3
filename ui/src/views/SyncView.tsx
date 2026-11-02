import { useEffect, useState } from 'react';
import { Pause, Play, RefreshCw, Wifi } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { NearbyDevice, SyncConflict, SyncSettings, SyncStatus } from '../types';
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

async function save(status: SyncStatus, patch: Partial<SyncSettings>): Promise<boolean> {
  const next = await attempt(api.setSyncSettings({ ...status.settings, ...patch }));
  if (next) useStore.getState().setSync(next);
  return !!next;
}

const PLATFORMS: Record<string, string> = {
  android: 'Android',
  linux: 'Linux',
  windows: 'Windows',
  macos: 'macOS',
};

const platformName = (p: string) => PLATFORMS[p] ?? p;

/// Other Cadenza devices on the network, asked for every couple of seconds while shown.
function useNearby(): NearbyDevice[] {
  const [nearby, setNearby] = useState<NearbyDevice[]>([]);
  useEffect(() => {
    let live = true;
    const look = () => void api.syncNearby().then((n) => live && setNearby(n));
    look();
    const timer = setInterval(look, 2000);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);
  return nearby;
}

/// Typing a pairing code: letters and digits in groups of four, as it's shown.
function formatCode(typed: string): string {
  const letters = typed
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, '')
    .slice(0, 12);
  return letters.match(/.{1,4}/g)?.join('-') ?? '';
}

/// Joining another device's library: its pairing code, and (when it wasn't found on the
/// network) its address.
function JoinForm({
  device,
  onDone,
}: {
  /** A device found on the network; none to type an address. */
  device: NearbyDevice | null;
  onDone: () => void;
}) {
  const [address, setAddress] = useState('');
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const ready = code.replace(/-/g, '').length === 12 && (device !== null || address.trim() !== '');
  return (
    <form
      className="folder-setup"
      onSubmit={(e) => {
        e.preventDefault();
        if (!ready || busy) return;
        setBusy(true);
        void attempt(api.syncJoin(device?.id ?? address.trim(), code)).then((next) => {
          setBusy(false);
          if (next) {
            useStore.getState().setSync(next);
            onDone();
          }
        });
      }}
    >
      {device ? (
        <p>
          Joining <strong>{device.name}</strong>. Type the pairing code shown on its Sync page.
        </p>
      ) : (
        <label className="field-row">
          <span>Its address</span>
          <input
            value={address}
            placeholder="192.168.1.20"
            inputMode="url"
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setAddress(e.target.value)}
          />
        </label>
      )}
      <label className="field-row">
        <span>Pairing code</span>
        <input
          className="pairing-input"
          value={code}
          placeholder="ABCD-EFGH-JK23"
          autoComplete="off"
          autoCapitalize="characters"
          spellCheck={false}
          onChange={(e) => setCode(formatCode(e.target.value))}
        />
      </label>
      <div className="row-actions">
        <button className="pill primary" type="submit" disabled={!ready || busy}>
          {busy ? 'Joining…' : 'Join'}
        </button>
        <button className="pill" type="button" onClick={onDone}>
          Cancel
        </button>
      </div>
    </form>
  );
}

/// Not syncing yet: start (the first device) or join a device that is.
function Setup({ status }: { status: SyncStatus }) {
  const nearby = useNearby();
  // Joining: a nearby device, or "address" to type one.
  const [joining, setJoining] = useState<NearbyDevice | 'address' | null>(null);
  const [busy, setBusy] = useState(false);
  const syncing = nearby.filter((d) => d.syncing);
  return (
    <>
      <h2 className="section-title">Join a Device</h2>
      {syncing.length === 0 ? (
        <p className="muted">
          No device that’s syncing is on this network yet. On your first device, choose Start
          Syncing below; then open this page on the others.
        </p>
      ) : (
        <ul className="history-list">
          {syncing.map((d) => (
            <li key={d.id}>
              <div>
                <span>{d.name}</span>
                <span className="muted small">
                  {platformName(d.platform)} · {d.address}
                </span>
              </div>
              <button className="pill primary" onClick={() => setJoining(d)}>
                Join…
              </button>
            </li>
          ))}
        </ul>
      )}
      {joining && (
        <JoinForm
          key={joining === 'address' ? 'address' : joining.id}
          device={joining === 'address' ? null : joining}
          onDone={() => setJoining(null)}
        />
      )}
      {joining === null && (
        <p className="muted small">
          Not listed? Some networks (guest Wi-Fi, some routers) hide devices from each other.{' '}
          <button className="link-button" onClick={() => setJoining('address')}>
            Enter its address
          </button>{' '}
          instead (shown on its Sync page).
        </p>
      )}

      <h2 className="section-title">Start Syncing</h2>
      <p className="muted">
        On the first device: this makes a pairing code that your other devices use to join.
      </p>
      <div className="row-actions">
        <button
          className="pill primary"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void attempt(api.syncStart()).then((next) => {
              setBusy(false);
              if (next) useStore.getState().setSync(next);
            });
          }}
        >
          <Wifi size={14} aria-hidden /> {busy ? 'Starting…' : 'Start Syncing'}
        </button>
      </div>
      {status.addresses.length > 0 && (
        <p className="muted small">This device’s address: {status.addresses.join(', ')}</p>
      )}
    </>
  );
}

/// Addresses typed in for devices that can't be found on this network.
function Addresses({ status }: { status: SyncStatus }) {
  const [adding, setAdding] = useState('');
  const list = status.settings.addresses;
  return (
    <details className="sync-problems">
      <summary>Devices added by address ({list.length})</summary>
      <ul>
        {list.map((a) => (
          <li key={a}>
            {a}{' '}
            <button
              className="link-button"
              onClick={() => void save(status, { addresses: list.filter((b) => b !== a) })}
            >
              Remove
            </button>
          </li>
        ))}
      </ul>
      <form
        className="field-row"
        onSubmit={(e) => {
          e.preventDefault();
          if (adding.trim())
            void save(status, { addresses: [...list, adding.trim()] }).then(
              (ok) => ok && setAdding(''),
            );
        }}
      >
        <input
          value={adding}
          placeholder="192.168.1.20"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => setAdding(e.target.value)}
        />
        <button className="pill" type="submit" disabled={!adding.trim()}>
          Add
        </button>
      </form>
    </details>
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

/// Sync: pairing with the owner's other devices on the same Wi-Fi, this device's name and
/// options, what happened, the other devices, and edits that clashed.
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
  const on = status.paired;
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
        Keeps tag edits, artwork, favourites, play counts and the Trash the same on your devices
        whenever they’re on the same Wi-Fi with Cadenza open. Nothing goes over the internet or
        through an account, and what passes between your devices is encrypted. Changes made while
        apart catch up the next time they meet.
      </p>

      <DeviceName key={deviceName} status={status} />

      {!on && <Setup status={status} />}

      {on && (
        <>
          <div className="pairing-card">
            <span className="muted small">Pairing code</span>
            <span className="pairing-code">{status.pairingCode}</span>
            <span className="muted small">
              To add a device: on the same Wi-Fi, open Cadenza’s Sync page there, choose this device
              ({deviceName}) and type this code.
              {status.addresses.length > 0 &&
                ` If it isn’t listed there, enter this device’s address: ${status.addresses.join(', ')}.`}
            </span>
          </div>
          <label className="check-row">
            <input
              type="checkbox"
              checked={status.settings.shareAudio}
              onChange={(e) => void save(status, { shareAudio: e.target.checked })}
            />
            Let my other devices copy songs from this one that they don’t have
          </label>
          <label className="check-row">
            <input
              type="checkbox"
              checked={status.settings.downloadAudio}
              onChange={(e) => void save(status, { downloadAudio: e.target.checked })}
            />
            Copy songs from my other devices that this one doesn’t have, into my first music folder
          </label>
          <Addresses status={status} />
          <div className="row-actions">
            <button
              className="pill"
              onClick={() => {
                if (
                  window.confirm(
                    'Stop syncing this device? Its music and edits stay as they are; the other devices carry on without it. To sync again, join one of them with its pairing code.',
                  )
                )
                  void attempt(api.syncLeave()).then((next) => {
                    if (next) useStore.getState().setSync(next);
                  });
              }}
            >
              Stop Syncing
            </button>
          </div>

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
                {t.direction === 'upload' ? 'Sending' : 'Copying from another device'}: {t.name} (
                {size(t.done)} of {size(t.total)})
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
            <p className="muted">Devices appear here once they’ve synced with this one.</p>
          ) : (
            <ul className="history-list">
              {status.devices.map((d) => (
                <li key={d.id}>
                  <div>
                    <span>
                      {!d.thisDevice && (
                        <span
                          className="online-dot"
                          data-on={d.online || undefined}
                          title={d.online ? 'On this network now' : 'Not reached in the last sync'}
                          aria-label={d.online ? 'Online' : 'Away'}
                        />
                      )}
                      {d.name}
                      {d.thisDevice && ' (this device)'}
                    </span>
                    <span className="muted small">
                      {platformName(d.platform)} · last change {when(d.lastPushMs)}
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
