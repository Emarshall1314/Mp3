import { useState } from 'react';
import type { TrackTags } from '../types';
import {
  type CaseStyle,
  type Changes,
  TEXT_FIELDS,
  changeCase,
  cleanup,
  clearField,
  copyField,
  findReplace,
  fixEncodings,
  joinValues,
  splitValues,
  tagsFromFilename,
} from '../tagActions';

const EXCEPTIONS_KEY = 'cadenza.caseExceptions';

function savedExceptions(): string {
  try {
    return localStorage.getItem(EXCEPTIONS_KEY) ?? 'AC/DC, DJ, MC, EP, LP, UK, USA, II, III, IV';
  } catch {
    return '';
  }
}

/// The actions: each works on every open song, on top of pending changes, and adds its result
/// to them for review (Changes tab) before anything is written.
export function TagActionsPanel({
  tags,
  pending,
  fieldOptions,
  apply,
}: {
  tags: TrackTags[];
  pending: Changes;
  fieldOptions: { field: string; label: string }[];
  apply: (changes: Changes, what: string) => void;
}) {
  const [message, setMessage] = useState<string | null>(null);
  const run = (what: string, f: () => Changes) => {
    try {
      const changes = f();
      const count = Object.values(changes).reduce((n, c) => n + Object.keys(c).length, 0);
      if (count === 0) setMessage(`${what}: nothing to change.`);
      else {
        apply(changes, what);
        setMessage(
          `${what}: ${count} ${count === 1 ? 'value' : 'values'} changed. Review them under Changes.`,
        );
      }
    } catch (e) {
      setMessage(`${what}: ${e instanceof Error ? e.message : String(e)}`);
    }
  };
  const textOnly = [{ field: '*', label: 'All text fields' }, ...fieldOptions];
  const scope = (f: string) => (f === '*' ? TEXT_FIELDS : [f]);

  const [fr, setFr] = useState({
    field: '*',
    find: '',
    replace: '',
    regex: false,
    matchCase: false,
  });
  const [cs, setCs] = useState<{ field: string; style: CaseStyle; keep: string }>({
    field: '*',
    style: 'title',
    keep: savedExceptions(),
  });
  const [cl, setCl] = useState({ spaces: true, trackPrefix: false, quotes: false });
  const [pattern, setPattern] = useState('%tracknumber% - %title%');
  const [cp, setCp] = useState({ from: 'artist', to: 'albumartist', onlyEmpty: true });
  const [sj, setSj] = useState({ field: 'artist', separator: ' feat. ' });
  const [clear, setClear] = useState('comment');
  const legacy = tags.flatMap((t) => t.legacy);

  const select = (
    value: string,
    onChange: (v: string) => void,
    label: string,
    options = fieldOptions,
  ) => (
    <select aria-label={label} value={value} onChange={(e) => onChange(e.target.value)}>
      {options.map((o) => (
        <option key={o.field} value={o.field}>
          {o.label}
        </option>
      ))}
    </select>
  );

  return (
    <div className="tag-actions">
      {message && (
        <p className="action-message" role="status">
          {message}
        </p>
      )}

      <section className="action">
        <h3>Find and Replace</h3>
        <div className="action-row">
          {select(fr.field, (field) => setFr({ ...fr, field }), 'Find in', textOnly)}
          <input
            type="text"
            aria-label="Find"
            placeholder="Find"
            value={fr.find}
            onChange={(e) => setFr({ ...fr, find: e.target.value })}
          />
          <input
            type="text"
            aria-label="Replace with"
            placeholder="Replace with"
            value={fr.replace}
            onChange={(e) => setFr({ ...fr, replace: e.target.value })}
          />
          <button
            className="pill"
            disabled={!fr.find}
            onClick={() =>
              run('Find and replace', () =>
                findReplace(tags, pending, { ...fr, fields: scope(fr.field) }),
              )
            }
          >
            Replace
          </button>
        </div>
        <div className="action-row">
          <label className="check-row">
            <input
              type="checkbox"
              checked={fr.matchCase}
              onChange={(e) => setFr({ ...fr, matchCase: e.target.checked })}
            />
            Match case
          </label>
          <label className="check-row">
            <input
              type="checkbox"
              checked={fr.regex}
              onChange={(e) => setFr({ ...fr, regex: e.target.checked })}
            />
            Regular expression ($1, $2 insert groups)
          </label>
        </div>
      </section>

      <section className="action">
        <h3>Change Case</h3>
        <div className="action-row">
          {select(cs.field, (field) => setCs({ ...cs, field }), 'Change case of', textOnly)}
          <select
            aria-label="Case"
            value={cs.style}
            onChange={(e) => setCs({ ...cs, style: e.target.value as CaseStyle })}
          >
            <option value="title">Title Case</option>
            <option value="sentence">Sentence case</option>
            <option value="upper">UPPER CASE</option>
            <option value="lower">lower case</option>
          </select>
          <button
            className="pill"
            onClick={() => {
              try {
                localStorage.setItem(EXCEPTIONS_KEY, cs.keep);
              } catch {
                // Not saved; the exceptions still apply now.
              }
              const keep = cs.keep
                .split(',')
                .map((k) => k.trim())
                .filter(Boolean);
              run('Change case', () => changeCase(tags, pending, cs.style, scope(cs.field), keep));
            }}
          >
            Apply
          </button>
        </div>
        <label className="action-row">
          <span className="muted small">Always written as:</span>
          <input
            type="text"
            aria-label="Words always written as given, separated by commas"
            value={cs.keep}
            onChange={(e) => setCs({ ...cs, keep: e.target.value })}
          />
        </label>
      </section>

      <section className="action">
        <h3>Clean Up</h3>
        <div className="action-row">
          <label className="check-row">
            <input
              type="checkbox"
              checked={cl.spaces}
              onChange={(e) => setCl({ ...cl, spaces: e.target.checked })}
            />
            Extra spaces
          </label>
          <label className="check-row">
            <input
              type="checkbox"
              checked={cl.trackPrefix}
              onChange={(e) => setCl({ ...cl, trackPrefix: e.target.checked })}
            />
            Track numbers in titles (“03 - Song”)
          </label>
          <label className="check-row">
            <input
              type="checkbox"
              checked={cl.quotes}
              onChange={(e) => setCl({ ...cl, quotes: e.target.checked })}
            />
            Typographic quotes
          </label>
          <button
            className="pill"
            onClick={() => run('Clean up', () => cleanup(tags, pending, cl, TEXT_FIELDS))}
          >
            Clean Up
          </button>
        </div>
      </section>

      <section className="action">
        <h3>Tags from File Names</h3>
        <div className="action-row">
          <input
            type="text"
            aria-label="File name pattern"
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
          />
          <button
            className="pill"
            onClick={() =>
              run('Tags from file names', () => {
                const { changes, matched } = tagsFromFilename(tags, pending, pattern);
                if (matched < tags.length)
                  setTimeout(() =>
                    setMessage(
                      (m) =>
                        `${m ?? ''} ${tags.length - matched} file names didn’t match and were left alone.`,
                    ),
                  );
                return changes;
              })
            }
          >
            Fill In
          </button>
        </div>
        <p className="muted small">
          Name fields like %title%, %artist%, %tracknumber%; use %skip% for parts to ignore. The
          extension is left out.
        </p>
      </section>

      <section className="action">
        <h3>Copy a Field</h3>
        <div className="action-row">
          {select(cp.from, (from) => setCp({ ...cp, from }), 'Copy from')}
          <span className="muted">to</span>
          {select(cp.to, (to) => setCp({ ...cp, to }), 'Copy to')}
          <label className="check-row">
            <input
              type="checkbox"
              checked={cp.onlyEmpty}
              onChange={(e) => setCp({ ...cp, onlyEmpty: e.target.checked })}
            />
            Only where empty
          </label>
          <button
            className="pill"
            disabled={cp.from === cp.to}
            onClick={() =>
              run('Copy', () => copyField(tags, pending, cp.from, cp.to, cp.onlyEmpty))
            }
          >
            Copy
          </button>
        </div>
      </section>

      <section className="action">
        <h3>Split or Join Values</h3>
        <div className="action-row">
          {select(sj.field, (field) => setSj({ ...sj, field }), 'Field')}
          <input
            type="text"
            aria-label="Separator"
            value={sj.separator}
            onChange={(e) => setSj({ ...sj, separator: e.target.value })}
          />
          <button
            className="pill"
            onClick={() => run('Split', () => splitValues(tags, pending, sj.field, sj.separator))}
          >
            Split
          </button>
          <button
            className="pill"
            onClick={() => run('Join', () => joinValues(tags, pending, sj.field, sj.separator))}
          >
            Join
          </button>
        </div>
        <p className="muted small">
          Split “A feat. B” into two artists, or join several genres into one value.
        </p>
      </section>

      <section className="action">
        <h3>Clear a Field</h3>
        <div className="action-row">
          {select(clear, setClear, 'Field to clear')}
          <button
            className="pill"
            onClick={() => run('Clear', () => clearField(tags, pending, clear))}
          >
            Clear
          </button>
        </div>
      </section>

      {legacy.length > 0 && (
        <section className="action">
          <h3>Fix Text Encoding</h3>
          <p className="small">
            Some text was saved in an old encoding and shows as gibberish, for example “
            {legacy[0]?.stored}” reads as “{legacy[0]?.decoded}” in {legacy[0]?.encoding}.
          </p>
          <button
            className="pill"
            onClick={() => run('Fix encoding', () => fixEncodings(tags, pending))}
          >
            Fix
          </button>
        </section>
      )}
    </div>
  );
}
