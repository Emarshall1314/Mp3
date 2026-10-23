import { describe, expect, it } from 'vitest';
import { artistLetters, swell } from './AlphabetIndex';

describe('the A–Z index', () => {
  it('files artists by first letter, "The …" under T and the next word', () => {
    expect(artistLetters('Coldplay')).toEqual(['C']);
    expect(artistLetters('The Beatles')).toEqual(['T', 'B']);
    expect(artistLetters('the national')).toEqual(['T', 'N']);
    expect(artistLetters('The The')).toEqual(['T']);
    expect(artistLetters('Theory of a Deadman')).toEqual(['T']);
    expect(artistLetters('Édith Piaf')).toEqual(['E']);
    expect(artistLetters('50 Cent')).toEqual(['#']);
    expect(artistLetters('  björk')).toEqual(['B']);
  });

  it('swells letters most under the finger, fading with distance', () => {
    expect(swell(0)).toBeCloseTo(1.9);
    expect(swell(1)).toBeGreaterThan(swell(2));
    expect(swell(6)).toBeCloseTo(1, 2);
  });
});
