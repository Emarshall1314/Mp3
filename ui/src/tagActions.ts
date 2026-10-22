// Tag editing actions: pure functions from songs' current values to new ones. The editor shows
// the result as changed cells to review before anything is written.
import type { TrackTags } from './types';

/// Pending text per song and field, as the editor holds it (multi-values joined with "; ").
export type Changes = Record<number, Record<string, string>>;

/// The fields actions work on unless told otherwise: the text people see and edit.
export const TEXT_FIELDS = [
  'title',
  'artist',
  'album',
  'albumartist',
  'composer',
  'genre',
  'grouping',
  'comment',
  'discsubtitle',
  'work',
  'movementname',
];

export const show = (values: string[] | undefined) => (values ?? []).join('; ');

/// A song's field as it stands with the pending changes.
export function current(t: TrackTags, changes: Changes, field: string): string {
  return changes[t.trackId]?.[field] ?? show(t.fields[field]);
}

/// Applies `f` to each song's value of each field; returns only values that change.
function mapFields(
  tags: TrackTags[],
  changes: Changes,
  fields: string[],
  f: (value: string, t: TrackTags, field: string) => string,
): Changes {
  const out: Changes = {};
  for (const t of tags) {
    for (const field of fields) {
      const before = current(t, changes, field);
      const after = f(before, t, field);
      if (after !== before) (out[t.trackId] ??= {})[field] = after;
    }
  }
  return out;
}

export interface FindReplace {
  find: string;
  replace: string;
  regex: boolean;
  matchCase: boolean;
  fields: string[];
}

/// Find and replace, as plain text or a regular expression ($1 for groups). Throws on an invalid
/// expression, with the browser's own message.
export function findReplace(tags: TrackTags[], changes: Changes, o: FindReplace): Changes {
  if (!o.find) return {};
  const source = o.regex ? o.find : o.find.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const re = new RegExp(source, o.matchCase ? 'g' : 'gi');
  const replacement = o.regex ? o.replace : o.replace.replace(/\$/g, '$$$$');
  return mapFields(tags, changes, o.fields, (v) => v.replace(re, replacement));
}

export type CaseStyle = 'title' | 'sentence' | 'upper' | 'lower';

/// Words kept lower case inside titles (English title case), unless first or last.
export const SMALL_WORDS = [
  'a',
  'an',
  'and',
  'as',
  'at',
  'but',
  'by',
  'for',
  'from',
  'in',
  'into',
  'nor',
  'of',
  'on',
  'or',
  'over',
  'the',
  'to',
  'up',
  'vs',
  'with',
];

export function changeCase(
  tags: TrackTags[],
  changes: Changes,
  style: CaseStyle,
  fields: string[],
  keep: string[] = [],
): Changes {
  return mapFields(tags, changes, fields, (v) => recase(v, style, keep));
}

/// `keep`: words written exactly as given wherever they appear ("AC/DC", "iPhone", "DJ").
export function recase(text: string, style: CaseStyle, keep: string[] = []): string {
  // Exceptions apply to the whole text afterwards, so ones with separators ("AC/DC") match.
  const withExceptions = (s: string) =>
    keep.reduce((out, k) => {
      const escaped = k.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      return out.replace(new RegExp(`(?<![\\p{L}\\p{N}])${escaped}(?![\\p{L}\\p{N}])`, 'giu'), k);
    }, s);
  if (style === 'upper') return withExceptions(text.toUpperCase());
  if (style === 'lower') return withExceptions(text.toLowerCase());
  // Words, with the separators between them preserved.
  const parts = text.split(/(\s+|(?<=[([/])|(?=[)\]/]))/);
  const isWord = (p: string | undefined) => !!p && !/^\s+$/.test(p);
  const wordIndexes = parts.flatMap((p, i) => (isWord(p) ? [i] : []));
  const firstIndex = wordIndexes[0];
  const lastIndex = wordIndexes.at(-1);
  let sentenceStart = true;
  const recased = parts
    .map((p, i) => {
      if (!isWord(p)) return p ?? '';
      const lower = p.toLowerCase();
      let out: string;
      if (style === 'sentence') out = sentenceStart ? capital(lower) : lower;
      else if (i !== firstIndex && i !== lastIndex && SMALL_WORDS.includes(lower)) out = lower;
      else out = capitalWords(lower);
      sentenceStart = /[.!?:]$/.test(p);
      return out;
    })
    .join('');
  return withExceptions(recased);
}

const capital = (w: string) => w.charAt(0).toLocaleUpperCase() + w.slice(1);
/// "rock-n-roll" → "Rock-N-Roll"; a leading quote or bracket is skipped.
const capitalWords = (w: string) =>
  w.replace(/(^|[-'"“‘([])(\p{L})/gu, (_, pre: string, c: string) => pre + c.toLocaleUpperCase());

export interface Cleanup {
  /// Trim, and collapse runs of spaces.
  spaces: boolean;
  /// "01 - Title", "01. Title", "1 Title" → "Title" (titles only).
  trackPrefix: boolean;
  /// Straight quotes and apostrophes to typographic ones.
  quotes: boolean;
}

export function cleanup(
  tags: TrackTags[],
  changes: Changes,
  o: Cleanup,
  fields: string[],
): Changes {
  return mapFields(tags, changes, fields, (v, _t, field) => {
    let s = v;
    if (o.spaces)
      s = s
        .replace(/[ \t]+/g, ' ')
        .trim()
        .replace(/ ;/g, ';');
    if (o.trackPrefix && field === 'title') s = s.replace(/^\d{1,3}\s*[-–._)]?\s+(?=\S)/, '');
    if (o.quotes)
      s = s
        .replace(/(^|[\s([{])"/g, '$1“')
        .replace(/"/g, '”')
        .replace(/(^|[\s([{])'/g, '$1‘')
        .replace(/'/g, '’');
    return s;
  });
}

/// Fills fields from each song's file name using a pattern such as "%tracknumber% - %title%".
/// Songs whose name doesn't match are left alone. Returns the changes and how many matched.
export function tagsFromFilename(
  tags: TrackTags[],
  changes: Changes,
  pattern: string,
): { changes: Changes; matched: number } {
  const names: string[] = [];
  const source = pattern
    .split(/(%[a-z_:]+%)/i)
    .map((part) => {
      const m = /^%([a-z_:]+)%$/i.exec(part);
      if (!m?.[1]) return part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      names.push(m[1].toLowerCase());
      return m[1].toLowerCase().includes('number') ? '(\\d+)' : '(.+?)';
    })
    .join('');
  const re = new RegExp(`^${source}$`);
  const out: Changes = {};
  let matched = 0;
  for (const t of tags) {
    const file = t.path.split(/[\\/]/).at(-1) ?? '';
    const stem = file.replace(/\.[^.]+$/, '');
    const m = re.exec(stem);
    if (!m) continue;
    matched++;
    names.forEach((field, i) => {
      const value = (m[i + 1] ?? '').trim();
      const normalised = field.includes('number') ? String(Number(value)) : value;
      if (field !== 'skip' && normalised !== current(t, changes, field))
        (out[t.trackId] ??= {})[field] = normalised;
    });
  }
  return { changes: out, matched };
}

/// Copies one field into another ("artist" → "albumartist"); `onlyEmpty` keeps existing values.
export function copyField(
  tags: TrackTags[],
  changes: Changes,
  from: string,
  to: string,
  onlyEmpty: boolean,
): Changes {
  return mapFields(tags, changes, [to], (v, t) => {
    if (onlyEmpty && v.trim()) return v;
    return current(t, changes, from);
  });
}

/// Splits a field's text into several values at `separator` (" feat. ", " & ").
export function splitValues(
  tags: TrackTags[],
  changes: Changes,
  field: string,
  separator: string,
): Changes {
  if (!separator) return {};
  return mapFields(tags, changes, [field], (v) =>
    v
      .split(';')
      .flatMap((part) => part.split(separator))
      .map((p) => p.trim())
      .filter(Boolean)
      .join('; '),
  );
}

/// Joins a field's values into one, with `separator`.
export function joinValues(
  tags: TrackTags[],
  changes: Changes,
  field: string,
  separator: string,
): Changes {
  return mapFields(tags, changes, [field], (v) =>
    v
      .split(';')
      .map((p) => p.trim())
      .filter(Boolean)
      .join(separator),
  );
}

export function clearField(tags: TrackTags[], changes: Changes, field: string): Changes {
  return mapFields(tags, changes, [field], () => '');
}

/// Text stored in a legacy 8-bit encoding (shown as gibberish) rewritten as the detected text.
export function fixEncodings(tags: TrackTags[], changes: Changes): Changes {
  const out: Changes = {};
  for (const t of tags) {
    if (!t.legacy.length) continue;
    for (const field of Object.keys(t.fields)) {
      const before = current(t, changes, field);
      let after = before;
      for (const l of t.legacy) after = after.split(l.stored).join(l.decoded);
      if (after !== before) (out[t.trackId] ??= {})[field] = after;
    }
  }
  return out;
}

/// Pending changes with an action's result laid over them.
export function merge(base: Changes, next: Changes): Changes {
  const out: Changes = { ...base };
  for (const [id, fields] of Object.entries(next)) {
    const key = Number(id);
    out[key] = { ...out[key], ...fields };
  }
  return out;
}
