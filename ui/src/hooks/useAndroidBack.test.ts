import { describe, expect, it } from 'vitest';
import { backAction } from './useAndroidBack';

const base = { menu: null, songInfo: null, nowPlaying: false, back: [], route: { name: 'home' } };

describe('Android back', () => {
  it('closes what is on top first, then goes back, then home, then leaves', () => {
    expect(backAction({ ...base, menu: {}, nowPlaying: true })).toBe('menu');
    expect(backAction({ ...base, songInfo: [1], nowPlaying: true })).toBe('songInfo');
    expect(backAction({ ...base, nowPlaying: true, back: [{}] })).toBe('nowPlaying');
    expect(backAction({ ...base, back: [{}] })).toBe('page');
    expect(backAction({ ...base, route: { name: 'albums' } })).toBe('home');
    expect(backAction(base)).toBe('background');
  });
});
