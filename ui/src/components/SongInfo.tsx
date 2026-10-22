import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { X } from 'lucide-react';
import { api } from '../api';
import { attempt, useStore } from '../store';
import type { Fields, PictureEdit, TrackRow, TrackTags } from '../types';
import { useModal } from '../hooks/useModal';
import { Artwork } from './Artwork';
import { TagActionsPanel } from './TagActionsPanel';
import { ArtworkEditor } from './ArtworkEditor';
import { Organiser } from './Organiser';

type Kind = 'text' | 'long' | 'number' | 'check';
interface FieldDef {
  field: string;
  label: string;
  kind?: Kind;
  /// Shown beside the previous field ("of" for track totals).
  joined?: string;
}

const DETAILS: FieldDef[] = [
  { field: 'title', label: 'Title' },
  { field: 'artist', label: 'Artist' },
  { field: 'album', label: 'Album' },
  { field: 'albumartist', label: 'Album Artist' },
  { field: 'composer', label: 'Composer' },
  { field: 'genre', label: 'Genre' },
  { field: 'date', label: 'Date' },
  { field: 'tracknumber', label: 'Track', kind: 'number' },
  { field: 'tracktotal', label: 'of', kind: 'number', joined: 'of' },
  { field: 'discnumber', label: 'Disc', kind: 'number' },
  { field: 'disctotal', label: 'of', kind: 'number', joined: 'of' },
  { field: 'compilation', label: 'Compilation', kind: 'check' },
  { field: 'grouping', label: 'Grouping' },
  { field: 'comment', label: 'Comment', kind: 'long' },
];

const CREDITS: FieldDef[] = [
  { field: 'lyricist', label: 'Lyricist' },
  { field: 'conductor', label: 'Conductor' },
  { field: 'arranger', label: 'Arranger' },
  { field: 'producer', label: 'Producer' },
  { field: 'engineer', label: 'Engineer' },
  { field: 'mixer', label: 'Mixer' },
  { field: 'label', label: 'Label' },
  { field: 'catalognumber', label: 'Catalogue Number' },
  { field: 'barcode', label: 'Barcode' },
  { field: 'isrc', label: 'ISRC' },
  { field: 'releasetype', label: 'Release Type' },
  { field: 'releasecountry', label: 'Release Country' },
  { field: 'media', label: 'Media' },
  { field: 'originaldate', label: 'Original Date' },
  { field: 'discsubtitle', label: 'Disc Subtitle' },
  { field: 'work', label: 'Work' },
  { field: 'movementname', label: 'Movement' },
  { field: 'movement', label: 'Movement No.', kind: 'number' },
  { field: 'movementtotal', label: 'of', kind: 'number', joined: 'of' },
  { field: 'bpm', label: 'BPM', kind: 'number' },
  { field: 'key', label: 'Key' },
  { field: 'mood', label: 'Mood' },
  { field: 'language', label: 'Language' },
  { field: 'copyright', label: 'Copyright' },
];

const SORTING: FieldDef[] = [
  { field: 'titlesort', label: 'Sort Title' },
  { field: 'artistsort', label: 'Sort Artist' },
  { field: 'albumsort', label: 'Sort Album' },
  { field: 'albumartistsort', label: 'Sort Album Artist' },
  { field: 'composersort', label: 'Sort Composer' },
];

const TABS = [
  'Details',
  'Credits',
  'Sorting',
  'Lyrics',
  'Artwork',
  'All Tags',
  'Table',
  'Actions',
  'Changes',
  'File',
] as const;
type Tab = (typeof TABS)[number];

/// Fields that hold several values, typed separated by ";".
const MULTI = new Set([
  'artist',
  'artists',
  'composer',
  'genre',
  'lyricist',
  'conductor',
  'arranger',
  'producer',
  'engineer',
  'mixer',
  'djmixer',
  'label',
  'releasetype',
  'mood',
  'musicbrainz_artistid',
  'musicbrainz_albumartistid',
]);
const isMulti = (field: string) =>
  MULTI.has(field) || field.startsWith('performer:') || field.startsWith('custom:');

/// Fields the editor never offers (Cadenza can't write them; see the tags crate).
const HIDDEN = new Set(['syncedlyrics', 'musicbrainz_recordingid']);

const show = (values: string[] | undefined) => (values ?? []).join('; ');
const parse = (field: string, text: string): string[] => {
  const parts = isMulti(field) ? text.split(';') : [text];
  return parts.map((p) => p.trim()).filter(Boolean);
};

const MIXED = Symbol('mixed');
type Shown = string | typeof MIXED;

/// Song Info: view and edit tags of one or more songs. Only fields you change are written.
export function SongInfo() {
  const ids = useStore((s) => s.songInfo);
  const close = useStore((s) => s.closeSongInfo);
  if (!ids) return null;
  return <SongInfoDialog key={ids.join(',')} ids={ids} onClose={close} />;
}

function SongInfoDialog({ ids, onClose }: { ids: number[]; onClose: () => void }) {
  const [tags, setTags] = useState<TrackTags[] | null>(null);
  const [rows, setRows] = useState<TrackRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>('Details');
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const [saving, setSaving] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    api.trackTags(ids).then(setTags, (e: unknown) => setError(String(e)));
    void api.tracks(ids).then(setRows);
  }, [ids]);

  /// Per-song edits from the Table tab, by track id.
  const [table, setTable] = useState<Record<number, Record<string, string>>>({});
  const [pictures, setPictures] = useState<PictureEdit | null>(null);
  const dirty = Object.keys(draft).length > 0 || Object.keys(table).length > 0 || pictures !== null;
  const escape = useCallback(() => {
    if (dirty) setConfirmDiscard(true);
    else onClose();
  }, [dirty, onClose]);
  useModal(true, ref, escape);
  useEffect(() => {
    if (confirmDiscard)
      ref.current?.querySelector<HTMLElement>('.sheet-footer [data-autofocus]')?.focus();
  }, [confirmDiscard]);

  /// A field's value across the songs: shared, or MIXED.
  const shown = useCallback(
    (field: string): Shown => {
      const head = tags?.[0];
      if (!tags || !head) return '';
      const first = show(head.fields[field]);
      return tags.every((t) => show(t.fields[field]) === first) ? first : MIXED;
    },
    [tags],
  );

  const set = (field: string, text: string) => {
    const original = shown(field);
    setDraft((d) => {
      const { [field]: _previous, ...rest } = d;
      return original !== MIXED && text === original ? rest : { ...rest, [field]: text };
    });
  };

  const setCell = (trackId: number, field: string, text: string) => {
    const original = show(tags?.find((t) => t.trackId === trackId)?.fields[field]);
    setTable((all) => {
      const { [field]: _previous, ...rest } = all[trackId] ?? {};
      const row = text === original ? rest : { ...rest, [field]: text };
      const { [trackId]: _old, ...others } = all;
      return Object.keys(row).length ? { ...others, [trackId]: row } : others;
    });
  };

  const numeric = (field: string) =>
    [...DETAILS, ...CREDITS].find((d) => d.field === field)?.kind === 'number';
  const bad = ([field, text]: [string, string]) =>
    numeric(field) && text.trim() !== '' && !/^\d+$/.test(text.trim());
  const invalid =
    Object.entries(draft).some(bad) ||
    Object.values(table).some((row) => Object.entries(row).some(bad));
  const editable = tags?.filter((t) => !t.readOnly) ?? [];

  /// Every song's pending text: the form's changes (all songs) under the table's (one song).
  const pending = useMemo(() => {
    const out: Record<number, Record<string, string>> = {};
    for (const t of tags ?? []) {
      const merged = { ...draft, ...table[t.trackId] };
      if (Object.keys(merged).length) out[t.trackId] = merged;
    }
    return out;
  }, [tags, draft, table]);

  /// An action's result: one song edits the form, several the table.
  const applyChanges = (changes: Record<number, Record<string, string>>) => {
    for (const [id, fields] of Object.entries(changes))
      for (const [field, text] of Object.entries(fields)) {
        if (ids.length === 1) set(field, text);
        else setCell(Number(id), field, text);
      }
  };

  const revert = (trackId: number, field: string) => {
    const original = show(tags?.find((t) => t.trackId === trackId)?.fields[field]);
    if (table[trackId]?.[field] !== undefined) setCell(trackId, field, original);
    else if (draft[field] !== undefined) {
      setDraft((d) => {
        const { [field]: _gone, ...rest } = d;
        return rest;
      });
    }
  };
  const changeCount =
    (pictures ? 1 : 0) +
    Object.entries(pending).reduce(
      (n, [id, fields]) =>
        n +
        Object.entries(fields).filter(
          ([f, text]) =>
            show(parse(f, text)) !== show(tags?.find((t) => t.trackId === Number(id))?.fields[f]),
        ).length,
      0,
    );

  const save = async () => {
    if (!dirty || invalid || !editable.length) return;
    const edits = editable
      .map((t) => {
        const fields: Fields = {};
        for (const [field, text] of Object.entries({ ...draft, ...table[t.trackId] }))
          fields[field] = parse(field, text);
        return pictures ? { trackId: t.trackId, fields, pictures } : { trackId: t.trackId, fields };
      })
      .filter((e) => Object.keys(e.fields).length > 0 || e.pictures);
    const name = rows[0]?.title;
    const description = ids.length === 1 && name ? `Edit “${name}”` : `Edit ${edits.length} songs`;
    setSaving(true);
    const batch = await attempt(api.editTracks(edits, description));
    setSaving(false);
    if (batch !== undefined) onClose();
  };

  const allFields = useMemo(() => {
    const names = new Set<string>();
    for (const t of tags ?? []) for (const f of Object.keys(t.fields)) names.add(f);
    for (const f of Object.keys(draft)) names.add(f);
    return [...names].filter((f) => !HIDDEN.has(f)).sort();
  }, [tags, draft]);

  const first = rows[0];
  const many = ids.length > 1;
  const readOnly = tags?.filter((t) => t.readOnly) ?? [];

  return (
    <div className="sheet-backdrop" onMouseDown={(e) => e.target === e.currentTarget && escape()}>
      <div
        ref={ref}
        className="sheet song-info"
        role="dialog"
        aria-modal="true"
        aria-label={many ? `Info for ${ids.length} songs` : 'Song Info'}
      >
        <header className="sheet-header">
          <Artwork hash={first?.artHash ?? null} size={64} alt="" />
          <div className="sheet-title">
            <h2>{many ? `${ids.length} songs` : (first?.title ?? 'Song Info')}</h2>
            <p className="muted">
              {many
                ? 'Fields showing “Mixed” differ between the songs; leave them to keep each as it is.'
                : [first?.artist, first?.album].filter(Boolean).join(' — ')}
            </p>
          </div>
          <button className="icon-button" aria-label="Close" onClick={escape}>
            <X size={18} />
          </button>
        </header>

        <div className="tabs" role="tablist">
          {TABS.filter((t) => t !== 'Table' || ids.length > 1).map((t) => (
            <button
              key={t}
              role="tab"
              aria-selected={tab === t}
              className={tab === t ? 'on' : ''}
              onClick={() => setTab(t)}
            >
              {t === 'Changes' && changeCount > 0 ? `Changes (${changeCount})` : t}
            </button>
          ))}
        </div>

        <div className="sheet-body" role="tabpanel" aria-label={tab}>
          {error && <p role="alert">{error}</p>}
          {!tags && !error && (
            <p className="muted" aria-busy="true">
              Reading tags…
            </p>
          )}
          {tags && tab === 'Details' && (
            <FieldGrid defs={DETAILS} shown={shown} draft={draft} set={set} />
          )}
          {tags && tab === 'Credits' && (
            <FieldGrid defs={CREDITS} shown={shown} draft={draft} set={set} />
          )}
          {tags && tab === 'Sorting' && (
            <>
              <p className="muted small">
                How names sort in lists, for example “Beatles, The” or a surname first.
              </p>
              <FieldGrid defs={SORTING} shown={shown} draft={draft} set={set} />
            </>
          )}
          {tags && tab === 'Lyrics' && (
            <>
              <FieldInput
                field="lyrics"
                label="Lyrics"
                kind="long"
                shown={shown('lyrics')}
                draft={draft.lyrics}
                set={set}
                rows={14}
              />
              <p className="muted small">
                Lyrics in the file, without timing. For lyrics that follow the song, put a .lrc file
                with the same name beside it.
              </p>
            </>
          )}
          {tags && tab === 'Table' && <TableEditor tags={tags} table={table} setCell={setCell} />}
          {tags && tab === 'Artwork' && (
            <ArtworkEditor tags={tags} edit={pictures} setEdit={setPictures} />
          )}
          {tags && tab === 'Actions' && (
            <TagActionsPanel
              tags={tags}
              pending={pending}
              fieldOptions={[...DETAILS, ...CREDITS, ...SORTING].filter((d) => !d.joined)}
              apply={applyChanges}
            />
          )}
          {tags && tab === 'Changes' && (
            <>
              {pictures && (
                <p className="changes-artwork">
                  Artwork:{' '}
                  {pictures.kind === 'removeAll'
                    ? 'all pictures removed'
                    : pictures.kind === 'setFront'
                      ? 'a new front cover'
                      : `${pictures.pictures.length} ${pictures.pictures.length === 1 ? 'picture' : 'pictures'} as arranged under Artwork`}{' '}
                  <button className="link" onClick={() => setPictures(null)}>
                    Revert
                  </button>
                </p>
              )}
              <ChangesList tags={tags} pending={pending} revert={revert} many={ids.length > 1} />
            </>
          )}
          {tags && tab === 'All Tags' && (
            <AllTags fields={allFields} shown={shown} draft={draft} set={set} />
          )}
          {tags && tab === 'File' && (
            <div className="file-info">
              {tags.map((t) => (
                <div key={t.trackId} className="file-row">
                  <code>{t.path}</code>
                  <span className="muted small">
                    Tags: {t.format}
                    {t.pictures.length > 0 &&
                      ` · ${t.pictures.length} ${t.pictures.length === 1 ? 'picture' : 'pictures'}`}
                  </span>
                  {t.readOnly && <span className="small warn">{t.readOnly}</span>}
                </div>
              ))}
              <Organiser ids={ids} onDone={onClose} />
            </div>
          )}
        </div>

        <footer className="sheet-footer">
          {confirmDiscard ? (
            <>
              <span>Discard your changes?</span>
              <button className="pill" onClick={() => setConfirmDiscard(false)} data-autofocus>
                Keep Editing
              </button>
              <button className="pill primary" onClick={onClose}>
                Discard
              </button>
            </>
          ) : (
            <>
              <span className="muted small">
                {readOnly.length > 0 &&
                  (readOnly.length === tags?.length
                    ? 'These tags can’t be changed (see File).'
                    : `${readOnly.length} of the songs can’t be changed and will be skipped (see File).`)}
                {invalid && ' Numbers only in number fields.'}
              </span>
              <button className="pill" onClick={escape}>
                Cancel
              </button>
              <button
                className="pill primary"
                disabled={!dirty || invalid || saving || !editable.length}
                onClick={() => void save()}
              >
                {saving ? 'Saving…' : 'Save'}
              </button>
            </>
          )}
        </footer>
      </div>
    </div>
  );
}

interface FieldProps {
  shown: (field: string) => Shown;
  draft: Record<string, string>;
  set: (field: string, text: string) => void;
}

function FieldGrid({ defs, shown, draft, set }: FieldProps & { defs: FieldDef[] }) {
  // Pairs ("Track 3 of 12") share a row.
  const rows: { head: FieldDef; rest: FieldDef[] }[] = [];
  for (const d of defs) {
    const last = rows.at(-1);
    if (d.joined && last) last.rest.push(d);
    else rows.push({ head: d, rest: [] });
  }
  return (
    <div className="form-grid">
      {rows.map(({ head, rest }) => (
        <div key={head.field} className="form-row">
          <label htmlFor={`f-${head.field}`}>{head.label}</label>
          <div className="form-inputs">
            <span className="form-input">
              <FieldInput
                field={head.field}
                label={head.label}
                kind={head.kind}
                shown={shown(head.field)}
                draft={draft[head.field]}
                set={set}
                id={`f-${head.field}`}
              />
            </span>
            {rest.map((d) => (
              <span key={d.field} className="form-input">
                <span className="muted">{d.joined}</span>
                <FieldInput
                  field={d.field}
                  label={`${head.label} total`}
                  kind={d.kind}
                  shown={shown(d.field)}
                  draft={draft[d.field]}
                  set={set}
                />
              </span>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function FieldInput({
  field,
  label,
  kind = 'text',
  shown,
  draft,
  set,
  id,
  rows = 3,
}: {
  field: string;
  label: string;
  kind?: Kind;
  shown: Shown;
  draft: string | undefined;
  set: (field: string, text: string) => void;
  id?: string;
  rows?: number;
}) {
  const mixed = shown === MIXED;
  const value = draft ?? (mixed ? '' : shown);
  const clearing = mixed && draft === '';
  const common = {
    id,
    'aria-label': label,
    placeholder: mixed ? 'Mixed' : undefined,
    title: isMulti(field) ? 'Separate several values with ;' : undefined,
    className: `${draft !== undefined ? 'changed' : ''} ${clearing ? 'clearing' : ''}`,
  };
  if (kind === 'check') {
    return (
      <input
        type="checkbox"
        aria-label={label}
        checked={value === '1'}
        ref={(el) => {
          if (el) el.indeterminate = mixed && draft === undefined;
        }}
        onChange={(e) => set(field, e.target.checked ? '1' : '')}
      />
    );
  }
  if (kind === 'long') {
    return (
      <textarea
        {...common}
        rows={rows}
        value={value}
        onChange={(e) => set(field, e.target.value)}
      />
    );
  }
  return (
    <input
      {...common}
      type="text"
      inputMode={kind === 'number' ? 'numeric' : undefined}
      size={kind === 'number' ? 4 : undefined}
      value={value}
      onChange={(e) => set(field, e.target.value)}
    />
  );
}

const COLUMNS: { field: string; label: string; narrow?: boolean }[] = [
  { field: 'tracknumber', label: '#', narrow: true },
  { field: 'title', label: 'Title' },
  { field: 'artist', label: 'Artist' },
  { field: 'album', label: 'Album' },
  { field: 'albumartist', label: 'Album Artist' },
  { field: 'discnumber', label: 'Disc', narrow: true },
  { field: 'date', label: 'Date', narrow: true },
  { field: 'genre', label: 'Genre' },
];

/// One row per song: edit cells directly, paste a block copied from a spreadsheet, fill a value
/// down a column, number the tracks in order.
function TableEditor({
  tags,
  table,
  setCell,
}: {
  tags: TrackTags[];
  table: Record<number, Record<string, string>>;
  setCell: (trackId: number, field: string, text: string) => void;
}) {
  const [focus, setFocus] = useState<{ row: number; col: number } | null>(null);
  const value = (t: TrackTags, field: string) => table[t.trackId]?.[field] ?? show(t.fields[field]);
  const cellId = (row: number, col: number) => `cell-${row}-${col}`;
  const move = (row: number, col: number) => document.getElementById(cellId(row, col))?.focus();

  const onKey = (e: React.KeyboardEvent, row: number, col: number) => {
    if (e.key === 'ArrowDown' || (e.key === 'Enter' && !e.shiftKey)) {
      e.preventDefault();
      move(row + 1, col);
    } else if (e.key === 'ArrowUp' || (e.key === 'Enter' && e.shiftKey)) {
      e.preventDefault();
      move(row - 1, col);
    } else if (e.key === 'd' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      fillDown(row, col);
    }
  };

  /// Several lines or tab-separated cells fill the block starting at this cell.
  const onPaste = (e: React.ClipboardEvent, row: number, col: number) => {
    const text = e.clipboardData.getData('text/plain').replace(/\r/g, '');
    if (!text.includes('\n') && !text.includes('\t')) return;
    e.preventDefault();
    const lines = text.split('\n');
    if (lines.at(-1) === '') lines.pop();
    lines.forEach((line, i) => {
      const t = tags[row + i];
      if (!t) return;
      line.split('\t').forEach((cell, j) => {
        const c = COLUMNS[col + j];
        if (c) setCell(t.trackId, c.field, cell);
      });
    });
  };

  const fillDown = (row: number, col: number) => {
    const c = COLUMNS[col];
    const source = tags[row];
    if (!c || !source) return;
    const v = value(source, c.field);
    for (const t of tags.slice(row + 1)) setCell(t.trackId, c.field, v);
  };

  const numberTracks = () => {
    tags.forEach((t, i) => setCell(t.trackId, 'tracknumber', String(i + 1)));
    for (const t of tags) setCell(t.trackId, 'tracktotal', String(tags.length));
  };

  return (
    <div className="table-editor">
      <div className="table-actions">
        <button
          className="pill"
          disabled={!focus || focus.row >= tags.length - 1}
          onClick={() => focus && fillDown(focus.row, focus.col)}
          title="Copy the selected cell to every row below it (Ctrl+D)"
        >
          Fill Down
        </button>
        <button
          className="pill"
          onClick={numberTracks}
          title="Number the songs 1, 2, 3… in this order"
        >
          Number Tracks
        </button>
        <span className="muted small">Paste a block copied from a spreadsheet into any cell.</span>
      </div>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              {COLUMNS.map((c) => (
                <th key={c.field} className={c.narrow ? 'narrow' : ''}>
                  {c.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {tags.map((t, row) => (
              <tr key={t.trackId} className={t.readOnly ? 'read-only' : ''}>
                {COLUMNS.map((c, col) => {
                  const changed = table[t.trackId]?.[c.field] !== undefined;
                  return (
                    <td key={c.field}>
                      <input
                        id={cellId(row, col)}
                        type="text"
                        aria-label={`${c.label}, row ${row + 1}`}
                        className={changed ? 'changed' : ''}
                        value={value(t, c.field)}
                        disabled={t.readOnly !== null}
                        onFocus={() => setFocus({ row, col })}
                        onChange={(e) => setCell(t.trackId, c.field, e.target.value)}
                        onKeyDown={(e) => onKey(e, row, col)}
                        onPaste={(e) => onPaste(e, row, col)}
                      />
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

/// Every pending change, song by song: what is written now and what will be.
function ChangesList({
  tags,
  pending,
  revert,
  many,
}: {
  tags: TrackTags[];
  pending: Record<number, Record<string, string>>;
  revert: (trackId: number, field: string) => void;
  many: boolean;
}) {
  const rows = tags.flatMap((t) =>
    Object.entries(pending[t.trackId] ?? {})
      .map(([field, text]) => ({
        t,
        field,
        before: show(t.fields[field]),
        after: show(parse(field, text)),
      }))
      .filter((r) => r.before !== r.after),
  );
  if (rows.length === 0) return <p className="muted">No changes to fields.</p>;
  return (
    <table className="changes-list">
      <tbody>
        {rows.map(({ t, field, before, after }) => (
          <tr key={`${t.trackId}-${field}`} className={t.readOnly ? 'read-only' : ''}>
            {many && <td className="song">{show(t.fields.title) || t.path.split('/').at(-1)}</td>}
            <th scope="row">
              <code>{field}</code>
            </th>
            <td className="before">{before || <span className="muted">(empty)</span>}</td>
            <td aria-hidden>→</td>
            <td className="after">{after || <span className="muted">(removed)</span>}</td>
            <td>
              <button className="link" onClick={() => revert(t.trackId, field)}>
                Revert
              </button>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/// Every field the songs have, by its raw name, plus adding one.
function AllTags({ fields, shown, draft, set }: FieldProps & { fields: string[] }) {
  const [name, setName] = useState('');
  const [value, setValue] = useState('');
  const add = () => {
    const n = name.trim();
    if (!n || !value.trim()) return;
    const known = /^[a-z_]+$/.test(n) || n.startsWith('performer:') || n.startsWith('custom:');
    set(known ? n : `custom:${n}`, value);
    setName('');
    setValue('');
  };
  return (
    <div className="all-tags">
      <table>
        <tbody>
          {fields.map((f) => {
            const s = shown(f);
            const removed = draft[f] === '';
            return (
              <tr key={f} className={removed ? 'removed' : ''}>
                <th scope="row">
                  <code>{f}</code>
                </th>
                <td>
                  <FieldInput
                    field={f}
                    label={f}
                    kind={f === 'lyrics' || f === 'comment' ? 'long' : 'text'}
                    shown={s}
                    draft={draft[f]}
                    set={set}
                    rows={2}
                  />
                </td>
                <td>
                  <button
                    className="icon-button small"
                    aria-label={`Remove ${f}`}
                    title="Remove"
                    onClick={() => set(f, '')}
                  >
                    <X size={12} />
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <div className="add-tag">
        <input
          aria-label="New tag name"
          placeholder="Name (e.g. ORCHESTRA)"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <input
          aria-label="New tag value"
          placeholder="Value"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
        />
        <button className="pill" onClick={add} disabled={!name.trim() || !value.trim()}>
          Add
        </button>
      </div>
      <p className="muted small">
        Names Cadenza doesn’t know are kept as they are, in the file’s own format.
      </p>
    </div>
  );
}

/// Lists the files a batch couldn't change, and why, until dismissed.
export function EditFailures() {
  const report = useStore((s) => s.editFailures);
  const dismiss = useStore((s) => s.setEditFailures);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => dismiss(null), [dismiss]);
  useModal(report !== null, ref, close);
  if (!report) return null;
  return (
    <div className="sheet-backdrop">
      <div
        ref={ref}
        className="sheet edit-failures"
        role="alertdialog"
        aria-modal="true"
        aria-label="Some tags weren’t saved"
      >
        <header className="sheet-header">
          <div className="sheet-title">
            <h2>
              {report.failed.length === 1
                ? 'One song wasn’t changed'
                : `${report.failed.length} songs weren’t changed`}
            </h2>
            <p className="muted">These files are exactly as they were.</p>
          </div>
        </header>
        <ul className="sheet-body failure-list">
          {report.failed.map((f) => (
            <li key={f.trackId}>
              <code>{f.path || `Song ${f.trackId}`}</code>
              <span>{f.error}</span>
            </li>
          ))}
        </ul>
        <footer className="sheet-footer">
          <span />
          <button className="pill primary" onClick={close}>
            OK
          </button>
        </footer>
      </div>
    </div>
  );
}
