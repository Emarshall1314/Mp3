import { describe, expect, it } from 'vitest';
import {
  changeCase,
  cleanup,
  copyField,
  findReplace,
  fixEncodings,
  joinValues,
  recase,
  splitValues,
  tagsFromFilename,
} from './tagActions';
import type { TrackTags } from './types';

const song = (id: number, fields: Record<string, string[]>, path = `/m/${id}.flac`): TrackTags => ({
  trackId: id,
  path,
  format: 'vorbis',
  fields,
  pictures: [],
  legacy: [],
  readOnly: null,
});

describe('tag actions', () => {
  it('finds and replaces text and patterns', () => {
    const tags = [song(1, { title: ['Live at the BBC'] }), song(2, { title: ['Studio'] })];
    const plain = findReplace(
      tags,
      {},
      {
        find: 'live',
        replace: 'Recorded',
        regex: false,
        matchCase: false,
        fields: ['title'],
      },
    );
    expect(plain).toEqual({ 1: { title: 'Recorded at the BBC' } });
    const re = findReplace(
      tags,
      {},
      {
        find: '^(\\w+) at the (\\w+)$',
        replace: '$2: $1',
        regex: true,
        matchCase: true,
        fields: ['title'],
      },
    );
    expect(re).toEqual({ 1: { title: 'BBC: Live' } });
    // "$" in plain replacement text is literal.
    const dollars = findReplace(
      tags,
      {},
      {
        find: 'Studio',
        replace: '$1 Studio',
        regex: false,
        matchCase: true,
        fields: ['title'],
      },
    );
    expect(dollars).toEqual({ 2: { title: '$1 Studio' } });
  });

  it('applies title, sentence, upper and lower case with exceptions', () => {
    expect(recase('the man in the high castle', 'title')).toBe('The Man in the High Castle');
    expect(recase('a tale of the city (live in london)', 'title')).toBe(
      'A Tale of the City (Live in London)',
    );
    expect(recase('rock-n-roll is here to stay', 'title')).toBe('Rock-N-Roll Is Here to Stay');
    expect(recase('back in black by ac/dc', 'title', ['AC/DC'])).toBe('Back in Black by AC/DC');
    expect(recase('HELLO WORLD. GOODBYE', 'sentence')).toBe('Hello world. Goodbye');
    expect(recase('Mixed Case', 'upper')).toBe('MIXED CASE');
    const tags = [song(1, { title: ['ALREADY'], album: ['some album'] })];
    expect(changeCase(tags, {}, 'title', ['album'])).toEqual({ 1: { album: 'Some Album' } });
  });

  it('cleans up spaces, track prefixes and quotes', () => {
    const tags = [song(1, { title: ["  03 - Don't  Stop  "], artist: ['A  B'] })];
    const out = cleanup(tags, {}, { spaces: true, trackPrefix: true, quotes: true }, [
      'title',
      'artist',
    ]);
    expect(out).toEqual({ 1: { title: 'Don’t Stop', artist: 'A B' } });
  });

  it('reads tags from file names', () => {
    const tags = [
      song(1, {}, '/music/Artist/03 - Third Song.flac'),
      song(2, {}, '/music/Artist/no pattern here.flac'),
    ];
    const { changes, matched } = tagsFromFilename(tags, {}, '%tracknumber% - %title%');
    expect(matched).toBe(1);
    expect(changes).toEqual({ 1: { tracknumber: '3', title: 'Third Song' } });
  });

  it('copies, splits and joins fields', () => {
    const tags = [
      song(1, { artist: ['A feat. B'] }),
      song(2, { artist: ['C'], albumartist: ['D'] }),
    ];
    expect(copyField(tags, {}, 'artist', 'albumartist', true)).toEqual({
      1: { albumartist: 'A feat. B' },
    });
    expect(splitValues(tags, {}, 'artist', ' feat. ')).toEqual({ 1: { artist: 'A; B' } });
    const joined = joinValues([song(3, { genre: ['Rock', 'Pop'] })], {}, 'genre', ' / ');
    expect(joined).toEqual({ 3: { genre: 'Rock / Pop' } });
  });

  it('works on pending changes, not only saved values', () => {
    const tags = [song(1, { title: ['x'] })];
    const pending = { 1: { title: 'the new title' } };
    expect(changeCase(tags, pending, 'title', ['title'])).toEqual({
      1: { title: 'The New Title' },
    });
  });

  it('fixes text stored in a legacy encoding', () => {
    const t = song(1, { title: ['ÈÎ'], album: ['ok'] });
    t.legacy = [{ stored: 'ÈÎ', decoded: 'ИО', encoding: 'windows-1251' }];
    expect(fixEncodings([t], {})).toEqual({ 1: { title: 'ИО' } });
  });
});
