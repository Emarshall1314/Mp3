import { describe, expect, it } from 'vitest';
import { type Body, LEAN, step } from './useDockPhysics';

const spring = { hz: 3, zeta: 0.3, drag: 1.6 };

function run(b: Body, page: number, ms: number) {
  let x = b;
  const path: number[] = [];
  for (let t = 0; t < ms; t += 16) {
    x = step(x, spring, page, 16);
    path.push(x.x);
  }
  return { body: x, path };
}

describe('the dock leaning with the page', () => {
  it('leans with the page in proportion to its speed, up to a limit', () => {
    expect(run({ x: 0, v: 0 }, -1, 3000).body.x).toBeCloseTo(-1.6, 1);
    expect(run({ x: 0, v: 0 }, -20, 3000).body.x).toBeCloseTo(-LEAN, 1);
  });

  it('springs back past its place, then settles', () => {
    const { path, body } = run({ x: -4, v: 0 }, 0, 3000);
    expect(Math.max(...path)).toBeGreaterThan(0.3); // the overshoot
    expect(Math.max(...path)).toBeLessThan(4);
    expect(Math.abs(body.x)).toBeLessThan(0.05);
  });
});
