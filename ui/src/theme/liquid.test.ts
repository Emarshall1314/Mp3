import { describe, expect, it } from 'vitest';
import { lensField } from './liquid';

const at = (f: Uint8ClampedArray, w: number, x: number, y: number) => {
  const i = (y * w + x) * 4;
  return [f[i] ?? 128, f[i + 1] ?? 128] as const;
};

describe('lensField', () => {
  const w = 200;
  const h = 60;
  const field = lensField(w, h, 30, 18);

  it('leaves the middle alone', () => {
    expect(at(field, w, 100, 30)).toEqual([128, 128]);
  });

  it('gathers inwards over the rim, most part-way in, and none at the very edge', () => {
    // A capsule's ends are round: at the left end's middle the edge faces left (x only).
    const row = [...Array(40).keys()].map((x) => at(field, w, x, 30)[0]);
    expect(row[0]).toBeLessThan(150);
    expect(Math.max(...row)).toBeGreaterThan(245);
    expect(row[30]).toBe(128);
    expect(Math.abs(at(field, w, 7, 30)[1] - 128)).toBeLessThan(12);
    // The right end mirrors the left.
    expect(at(field, w, 199 - 7, 30)[0]).toBeLessThan(15);
    // Top and bottom edges move in y.
    expect(at(field, w, 100, 7)[0]).toBe(128);
    expect(at(field, w, 100, 7)[1]).toBeGreaterThan(240);
    expect(at(field, w, 100, 52)[1]).toBeLessThan(16);
  });

  it('never folds: every point of the rim shows something further in than the one outside it', () => {
    // Where a pixel samples from: x + offset, offset = scale × (value / 255 − ½).
    const reach = 0.385 * 18;
    const from = [...Array(30).keys()].map(
      (x) => x + 0.5 + 2 * reach * ((at(field, w, x, 30)[0] - 128) / 254),
    );
    for (let x = 1; x < from.length; x++) expect(from[x]).toBeGreaterThan(from[x - 1] ?? 0);
  });

  it('follows the curve of a corner', () => {
    const [x, y] = at(field, w, 12, 12);
    expect(x).toBeGreaterThan(128);
    expect(y).toBeGreaterThan(128);
  });
});
