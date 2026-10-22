import { useState } from 'react';
import { ArrowDown, ArrowUp, Trash2 } from 'lucide-react';
import { isAndroid } from '../api';
import { updateLayout, updateLook, useLook } from '../hooks/useAppearance';
import {
  BUILT_IN_PRESETS,
  COLUMN_SPECS,
  CONTROL_LABELS,
  type ColumnId,
  DEFAULT_LAYOUT,
  type GridOrList,
  type Layout,
  NAV_LABELS,
  type NavId,
  type PlayerControl,
  SHELF_LABELS,
  type ShelfId,
  move,
} from '../theme/layout';
import { useStore } from '../store';

function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map(([v, text]) => (
        <button
          key={v}
          role="radio"
          aria-checked={value === v}
          className={value === v ? 'on' : ''}
          onClick={() => onChange(v)}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

/// An ordered list where items can be shown, hidden and moved up or down.
function OrderList<T extends string>({
  label,
  all,
  shown,
  names,
  locked = [],
  onChange,
}: {
  label: string;
  all: T[];
  shown: T[];
  names: Record<T, string>;
  locked?: T[];
  onChange: (shown: T[]) => void;
}) {
  const hidden = all.filter((i) => !shown.includes(i));
  return (
    <ul className="order-list" aria-label={label}>
      {[...shown, ...hidden].map((id) => {
        const index = shown.indexOf(id);
        const on = index >= 0;
        return (
          <li key={id}>
            <label className="check-row">
              <input
                type="checkbox"
                checked={on}
                disabled={locked.includes(id)}
                onChange={(e) =>
                  onChange(e.target.checked ? [...shown, id] : shown.filter((s) => s !== id))
                }
              />
              {names[id]}
            </label>
            <button
              className="icon-button small"
              aria-label={`Move ${names[id]} up`}
              disabled={!on || index === 0}
              onClick={() => onChange(move(shown, index, -1))}
            >
              <ArrowUp size={14} />
            </button>
            <button
              className="icon-button small"
              aria-label={`Move ${names[id]} down`}
              disabled={!on || index === shown.length - 1}
              onClick={() => onChange(move(shown, index, 1))}
            >
              <ArrowDown size={14} />
            </button>
          </li>
        );
      })}
    </ul>
  );
}

const ALL_NAV = Object.keys(NAV_LABELS) as NavId[];
/// Controls on the right of the bar, in an order the owner picks; shuffle and repeat sit
/// either side of the transport buttons.
const RIGHT_CONTROLS = (['volume', 'output', 'miniPlayer', 'queue'] as PlayerControl[]).filter(
  (c) => !(isAndroid && c === 'miniPlayer'),
);
const ALL_SHELVES = Object.keys(SHELF_LABELS) as ShelfId[];
const ALL_COLUMNS = Object.keys(COLUMN_SPECS) as ColumnId[];
const COLUMN_NAMES = Object.fromEntries(
  ALL_COLUMNS.map((c) => [c, COLUMN_SPECS[c].label]),
) as Record<ColumnId, string>;

/// Applies a named layout.
export function applyPreset(layout: Layout) {
  updateLayout(layout);
}

export const allPresets = () => [...BUILT_IN_PRESETS, ...useLook.getState().look.presets];

/// Switches to the next named layout (after the one in use, if it is one).
export function cyclePreset() {
  const presets = allPresets();
  const now = JSON.stringify(useLook.getState().look.layout);
  const at = presets.findIndex((p) => JSON.stringify(p.layout) === now);
  const next = presets[(at + 1) % presets.length];
  if (!next) return;
  applyPreset(next.layout);
  useStore.getState().showToast(`Layout: ${next.name}`);
}

/// The layout menu's items, for the toolbar button.
export function presetMenu() {
  return allPresets().map((p) => ({ label: p.name, run: () => applyPreset(p.layout) }));
}

/// Layout: presets, sidebar, player bar, default views, song columns and Now Playing.
export function LayoutSettings() {
  const look = useLook((s) => s.look);
  const layout = look.layout;
  const showToast = useStore((s) => s.showToast);
  const [presetName, setPresetName] = useState('');
  const views = (key: keyof Layout['views']) => (v: GridOrList) =>
    updateLayout({ views: { ...layout.views, [key]: v } });

  const savePreset = () => {
    const name = presetName.trim();
    if (!name) return;
    const presets = [...look.presets.filter((p) => p.name !== name), { name, layout }];
    updateLook({ presets });
    setPresetName('');
    showToast(`Saved the layout “${name}”.`);
  };

  return (
    <>
      <div className="settings-group">
        <h2>Layout Presets</h2>
        <div className="action-row">
          {[...BUILT_IN_PRESETS, ...look.presets].map((p, i) => (
            <span key={`${i}-${p.name}`} className="preset">
              <button className="pill" onClick={() => applyPreset(p.layout)}>
                {p.name}
              </button>
              {i >= BUILT_IN_PRESETS.length && (
                <button
                  className="icon-button small"
                  aria-label={`Delete layout ${p.name}`}
                  onClick={() => updateLook({ presets: look.presets.filter((o) => o !== p) })}
                >
                  <Trash2 size={12} />
                </button>
              )}
            </span>
          ))}
        </div>
        <div className="action-row">
          <input
            type="text"
            aria-label="Name for this layout"
            placeholder="Name for this layout"
            maxLength={40}
            value={presetName}
            onChange={(e) => setPresetName(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && savePreset()}
          />
          <button className="pill" disabled={!presetName.trim()} onClick={savePreset}>
            Save Current Layout
          </button>
        </div>
        <p className="muted small">
          Switch layouts from the layout button above every page, or with Ctrl+Shift+L.
        </p>
      </div>

      <div className="settings-group">
        <h2>Sidebar</h2>
        <OrderList
          label="Sidebar items"
          all={ALL_NAV}
          shown={layout.nav}
          names={NAV_LABELS}
          onChange={(nav) => updateLayout({ nav })}
        />
        <label className="check-row">
          <input
            type="checkbox"
            checked={layout.sidebarCollapsed}
            onChange={(e) => updateLayout({ sidebarCollapsed: e.target.checked })}
          />
          Icons only
        </label>
        <p className="muted small">
          Drag the sidebar’s edge to resize it; double-click the edge to switch to icons only.
          Sound, Music Folders and Settings always stay at the bottom.
        </p>
      </div>

      <div className="settings-group">
        <h2>Home</h2>
        <OrderList
          label="Home shelves"
          all={ALL_SHELVES}
          shown={layout.shelves}
          names={SHELF_LABELS}
          onChange={(shelves) => updateLayout({ shelves })}
        />
      </div>

      <div className="settings-group">
        <h2>Player Bar</h2>
        <div className="form-grid">
          <span>Position</span>
          <Segmented
            label="Player bar position"
            value={layout.playerPosition}
            options={[
              ['bottom', 'Bottom'],
              ['top', 'Top'],
            ]}
            onChange={(playerPosition) => updateLayout({ playerPosition })}
          />
          <span>Size</span>
          <Segmented
            label="Player bar size"
            value={layout.playerSize}
            options={[
              ['compact', 'Compact'],
              ['standard', 'Standard'],
              ['large', 'Large'],
            ]}
            onChange={(playerSize) => updateLayout({ playerSize })}
          />
        </div>
        {(['shuffle', 'repeat'] as PlayerControl[]).map((c) => (
          <label key={c} className="check-row">
            <input
              type="checkbox"
              checked={layout.playerControls.includes(c)}
              onChange={(e) =>
                updateLayout({
                  playerControls: e.target.checked
                    ? [...layout.playerControls, c]
                    : layout.playerControls.filter((o) => o !== c),
                })
              }
            />
            {CONTROL_LABELS[c]}
          </label>
        ))}
        <OrderList
          label="Controls on the right"
          all={RIGHT_CONTROLS}
          shown={layout.playerControls.filter((c) => RIGHT_CONTROLS.includes(c))}
          names={CONTROL_LABELS}
          onChange={(right) =>
            updateLayout({
              playerControls: [
                ...layout.playerControls.filter((c) => c === 'shuffle' || c === 'repeat'),
                ...right,
              ],
            })
          }
        />
        <p className="muted small">
          Play, previous, next and the position bar are always there. The controls on the right
          appear in this order.
        </p>
      </div>

      <div className="settings-group">
        <h2>Views</h2>
        <div className="form-grid">
          <span>Albums</span>
          <Segmented
            label="Albums view"
            value={layout.views.albums}
            options={[
              ['grid', 'Grid'],
              ['list', 'List'],
            ]}
            onChange={views('albums')}
          />
          <span>Recently Added</span>
          <Segmented
            label="Recently Added view"
            value={layout.views.recent}
            options={[
              ['grid', 'Grid'],
              ['list', 'List'],
            ]}
            onChange={views('recent')}
          />
          <span>Artists</span>
          <Segmented
            label="Artists view"
            value={layout.views.artists}
            options={[
              ['list', 'List'],
              ['grid', 'Grid'],
            ]}
            onChange={views('artists')}
          />
          <span>Side panel</span>
          <Segmented
            label="Side panel position"
            value={layout.dock.position}
            options={[
              ['right', 'Right'],
              ['left', 'Left'],
              ['bottom', 'Bottom'],
              ['floating', 'Floating'],
            ]}
            onChange={(position) => updateLayout({ dock: { ...layout.dock, position } })}
          />
          <span>Now Playing</span>
          <Segmented
            label="Now Playing style"
            value={layout.nowPlaying}
            options={[
              ['artwork', 'Artwork'],
              ['lyrics', 'Lyrics First'],
              ['minimal', 'Minimal'],
            ]}
            onChange={(nowPlaying) => updateLayout({ nowPlaying })}
          />
        </div>
      </div>

      <div className="settings-group">
        <h2>Song List Columns</h2>
        <OrderList
          label="Song list columns"
          all={ALL_COLUMNS}
          shown={layout.columns.map((c) => c.id)}
          names={COLUMN_NAMES}
          locked={['title']}
          onChange={(ids) =>
            updateLayout({
              columns: ids.map(
                (id) =>
                  layout.columns.find((c) => c.id === id) ?? { id, width: COLUMN_SPECS[id].width },
              ),
            })
          }
        />
        <p className="muted small">
          In the song list, drag a column heading to move it and drag its edge to resize it.
        </p>
        <div className="action-row">
          <button className="pill" onClick={() => updateLayout(DEFAULT_LAYOUT)}>
            Reset Layout
          </button>
        </div>
      </div>
    </>
  );
}
