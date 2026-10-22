import { useState } from 'react';
import { api } from '../api';
import { attempt } from '../store';
import type { RenamePlan } from '../types';

const PATTERN_KEY = 'cadenza.organisePattern';

export const PRESETS: { label: string; pattern: string }[] = [
  {
    label: 'Artist / Year - Album / 01 Title',
    pattern: '%albumartist%/[%year% - ]%album%/[%discnumber%-]%tracknumber:2% %title%',
  },
  { label: 'Artist / Album / 01 Title', pattern: '%albumartist%/%album%/%tracknumber:2% %title%' },
  {
    label: 'Artist - Album / 01 - Artist - Title',
    pattern: '%albumartist% - %album%/%tracknumber:2% - %artist% - %title%',
  },
];

function savedPattern(): string {
  try {
    return localStorage.getItem(PATTERN_KEY) ?? PRESETS[0]?.pattern ?? '';
  } catch {
    return PRESETS[0]?.pattern ?? '';
  }
}

/// Renames and moves the songs' files from their tags, inside their music folder, after a
/// preview. Undoable from Music Folders, like tag changes.
export function Organiser({ ids, onDone }: { ids: number[]; onDone: () => void }) {
  const [pattern, setPattern] = useState(savedPattern);
  const [plan, setPlan] = useState<RenamePlan[] | null>(null);
  const [busy, setBusy] = useState(false);
  const moves = plan?.filter((p) => p.status === 'move').length ?? 0;

  const preview = async () => {
    try {
      localStorage.setItem(PATTERN_KEY, pattern);
    } catch {
      // Not remembered; the preview still runs.
    }
    setBusy(true);
    setPlan((await attempt(api.previewOrganise(ids, pattern))) ?? null);
    setBusy(false);
  };

  const move = async () => {
    setBusy(true);
    const batch = await attempt(api.organise(ids, pattern));
    setBusy(false);
    if (batch !== undefined) onDone();
  };

  return (
    <div className="organiser">
      <h3>Rename Files from Tags</h3>
      <div className="action-row">
        <select
          aria-label="Preset"
          value=""
          onChange={(e) => {
            setPattern(e.target.value);
            setPlan(null);
          }}
        >
          <option value="" disabled>
            Presets…
          </option>
          {PRESETS.map((p) => (
            <option key={p.pattern} value={p.pattern}>
              {p.label}
            </option>
          ))}
        </select>
        <input
          type="text"
          aria-label="Name pattern"
          value={pattern}
          onChange={(e) => {
            setPattern(e.target.value);
            setPlan(null);
          }}
        />
        <button className="pill" disabled={busy || !pattern.trim()} onClick={() => void preview()}>
          Preview
        </button>
      </div>
      <p className="muted small">
        Inside each song’s music folder. %field% fills in a field, %tracknumber:2% pads to two
        digits, %year% is the date’s year, and [ ] leaves a part out when a field in it is empty.
        Characters other systems can’t store are replaced.
      </p>
      {plan && (
        <>
          <table className="changes-list rename-plan">
            <tbody>
              {plan.map((p) => (
                <tr key={p.trackId} className={p.status}>
                  <td className="before">{p.from.split('/').slice(-3).join('/')}</td>
                  <td aria-hidden>→</td>
                  <td className="after">
                    {p.status === 'move' || p.status === 'conflict'
                      ? p.to.split('/').slice(-3).join('/')
                      : ''}
                    {p.status === 'same' && <span className="muted">already named this way</span>}
                    {p.note && <span className="small warn"> {p.note}</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="action-row">
            <button
              className="pill primary"
              disabled={busy || moves === 0}
              onClick={() => void move()}
            >
              {moves === 1 ? 'Move 1 File' : `Move ${moves} Files`}
            </button>
            <span className="muted small">
              Play counts and ratings stay with the songs. A .lrc beside a song moves with it.
            </span>
          </div>
        </>
      )}
    </div>
  );
}
